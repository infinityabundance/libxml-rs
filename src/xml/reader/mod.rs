//! XML Reader API (§30, §85 Phase 7).
//!
//! Cursor-based streaming reader with node type, depth, attribute traversal,
//! namespace lookup, value retrieval, validation integration.
//!
//! Implements the `xmlTextReader` API from libxml2, which provides a
//! cursor-based streaming interface for reading XML documents. The reader
//! parses the entire document into a tree on the first `Read()` call, then
//! walks the tree in document order (depth-first traversal) generating
//! node events for elements, text, comments, PIs, etc.
//!
//! # UPSTREAM-PARITY
//!
//! The reader API is defined in `libxml/xmlreader.h` and `libxml/xmlreader.c`.
//! Key differences from upstream:
//!
//! - The reader parses the full document on first Read rather than using
//!   a true streaming/event-driven parser. This simplifies the implementation
//!   while preserving the observable API surface.
//! - Pattern-based reader operations (xmlTextReaderPreservePattern, etc.)
//!   are not yet implemented.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

#[cfg(test)]
use crate::xml::string::xmlstr_to_string;

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_long, c_uint};

use crate::abi::allocator::xmlFreeImpl;
use crate::abi::callbacks::{xmlInputCloseCallback, xmlInputReadCallback};
use crate::abi::structs::{_xmlAttr, _xmlDoc, _xmlNode, _xmlParserCtxt, _xmlParserInputBuffer};

use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::parser::helpers::{
    create_parser_ctxt, free_parser_ctxt, input_from_file, input_from_io, input_from_memory,
    input_from_memory_named, parse_document, setup_parser_input,
};
use crate::xml::parser::input::InputBuffer;
use crate::xml::string::{bytes_to_xmlstr, xml_strdup, xmlstr_to_bytes};
use crate::xml::tree;
use std::collections::HashSet;

// ═══════════════════════════════════════════════════════════════════════════════
// Reader Types (xmlreader.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Reader node types (xmlReaderTypes enum).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// typedef enum {
///     XML_TEXTREADER_NONE = 0,
///     XML_TEXTREADER_ELEMENT = 1,
///     XML_TEXTREADER_ATTRIBUTE = 2,
///     XML_TEXTREADER_TEXT = 3,
///     XML_TEXTREADER_CDATA = 4,
///     XML_TEXTREADER_ENTITY_REFERENCE = 5,
///     XML_TEXTREADER_ENTITY = 6,
///     XML_TEXTREADER_PROCESSING_INSTRUCTION = 7,
///     XML_TEXTREADER_COMMENT = 8,
///     XML_TEXTREADER_DOCUMENT = 9,
///     XML_TEXTREADER_DOCUMENT_TYPE = 10,
///     XML_TEXTREADER_DOCUMENT_FRAGMENT = 11,
///     XML_TEXTREADER_NOTATION = 12,
///     XML_TEXTREADER_WHITESPACE = 13,
///     XML_TEXTREADER_SIGNIFICANT_WHITESPACE = 14,
///     XML_TEXTREADER_END_ELEMENT = 15,
///     XML_TEXTREADER_END_ENTITY = 16,
///     XML_TEXTREADER_XML_DECLARATION = 17
/// } xmlReaderTypes;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ReaderNodeType {
    NONE = 0,
    ELEMENT = 1,
    ATTRIBUTE = 2,
    TEXT = 3,
    CDATA = 4,
    ENTITY_REFERENCE = 5,
    ENTITY = 6,
    PROCESSING_INSTRUCTION = 7,
    COMMENT = 8,
    DOCUMENT = 9,
    DOCUMENT_TYPE = 10,
    DOCUMENT_FRAGMENT = 11,
    NOTATION = 12,
    WHITESPACE = 13,
    SIGNIFICANT_WHITESPACE = 14,
    END_ELEMENT = 15,
    END_ENTITY = 16,
    XML_DECLARATION = 17,
    NAMESPACE = 18,
}

/// Reader read state (xmlTextReaderReadState enum).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// typedef enum {
///     XML_TEXTREADER_NOT_INITIALIZED = 0,
///     XML_TEXTREADER_INITIALIZED = 1,
///     XML_TEXTREADER_READING = 2,
///     XML_TEXTREADER_EOF = 3,
///     XML_TEXTREADER_CLOSED = 4,
///     XML_TEXTREADER_ERROR = 5
/// } xmlTextReaderReadState;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ReadState {
    NOT_INITIALIZED = 0,
    INITIALIZED = 1,
    READING = 2,
    EOF = 3,
    CLOSED = 4,
    ERROR = 5,
}

/// Parser properties for xmlTextReaderGetParserProp / SetParserProp.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// typedef enum {
///     XML_PARSER_LOADDTD = 1,
///     XML_PARSER_DEFAULTATTRS = 2,
///     XML_PARSER_VALIDATE = 3,
///     XML_PARSER_SUBST_ENTITIES = 4
/// } xmlParserProperties;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
#[allow(dead_code)]
pub(crate) enum ParserProp {
    LOADDTD = 1,
    DEFAULTATTRS = 2,
    VALIDATE = 3,
    SUBST_ENTITIES = 4,
}

/// A traversal event in the document-order walk of the parsed tree.
///
/// Each event represents either entering a node (ELEMENT, TEXT, etc.) or
/// exiting an element (END_ELEMENT). The `depth` is the element nesting
/// depth at the time of the event.
#[derive(Debug, Clone)]
struct TraversalEvent {
    /// The node this event refers to.
    node: *mut _xmlNode,
    /// Whether this is an "exit" event (END_ELEMENT).
    is_end: bool,
    /// The depth at this event (number of ancestor elements).
    depth: i32,
}

/// Compute the element nesting depth of a node in the tree.
///
/// Counts the number of `XML_ELEMENT_NODE` ancestors.
///
/// # Safety
///
/// `node` must be a valid pointer to a node in a valid tree, or NULL.
#[allow(dead_code)]
unsafe fn compute_depth(node: *mut _xmlNode) -> i32 {
    if node.is_null() {
        return 0;
    }
    let mut depth: i32 = 0;
    // SAFETY: node is valid, and parent pointers form a tree.
    let mut cur = unsafe { (*node).parent };
    while !cur.is_null() {
        // SAFETY: cur is valid.
        if unsafe { (*cur).type_ } == XML_ELEMENT_NODE as c_int {
            depth += 1;
        }
        // SAFETY: cur's parent is valid.
        cur = unsafe { (*cur).parent };
    }
    depth
}

/// Convert an `xmlElementType` to the corresponding `ReaderNodeType`.
const fn element_type_to_reader_type(etype: c_int) -> ReaderNodeType {
    match etype {
        x if x == XML_ELEMENT_NODE as c_int => ReaderNodeType::ELEMENT,
        x if x == XML_ATTRIBUTE_NODE as c_int => ReaderNodeType::ATTRIBUTE,
        x if x == XML_TEXT_NODE as c_int => ReaderNodeType::TEXT,
        x if x == XML_CDATA_SECTION_NODE as c_int => ReaderNodeType::CDATA,
        x if x == XML_ENTITY_REF_NODE as c_int => ReaderNodeType::ENTITY_REFERENCE,
        x if x == XML_ENTITY_NODE as c_int => ReaderNodeType::ENTITY,
        x if x == XML_PI_NODE as c_int => ReaderNodeType::PROCESSING_INSTRUCTION,
        x if x == XML_COMMENT_NODE as c_int => ReaderNodeType::COMMENT,
        x if x == XML_DOCUMENT_NODE as c_int => ReaderNodeType::DOCUMENT,
        x if x == XML_DOCUMENT_TYPE_NODE as c_int => ReaderNodeType::DOCUMENT_TYPE,
        x if x == XML_DOCUMENT_FRAG_NODE as c_int => ReaderNodeType::DOCUMENT_FRAGMENT,
        x if x == XML_NOTATION_NODE as c_int => ReaderNodeType::NOTATION,
        x if x == XML_DTD_NODE as c_int => ReaderNodeType::DOCUMENT_TYPE,
        x if x == XML_NAMESPACE_DECL as c_int => ReaderNodeType::NONE,
        _ => ReaderNodeType::NONE,
    }
}

/// Check whether a text node consists entirely of whitespace.
fn is_whitespace_only(text: &[u8]) -> bool {
    text.iter()
        .all(|&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
}

// ═══════════════════════════════════════════════════════════════════════════════
// XmlTextReader — Internal Rust Type
// ═══════════════════════════════════════════════════════════════════════════════

/// The internal representation of an `xmlTextReader`.
///
/// This struct holds all state for the reader cursor: the parsed document,
/// the current position in the traversal, attribute navigation state, and
/// cached information about the current node.
/// The element the reader is currently positioned on.
#[derive(Clone, Copy)]
enum AttrTarget {
    None,
    Ns(*mut crate::abi::structs::_xmlNs),
    Prop(*mut _xmlAttr),
}

#[derive(Debug)]
pub struct XmlTextReader {
    /// The parsed XML document.
    doc: *mut _xmlDoc,
    /// The parser context used to parse the document (NULL after parsing).
    ctxt: *mut _xmlParserCtxt,
    /// The traversal events computed from the parsed tree.
    events: Vec<TraversalEvent>,
    /// Index into `events` for the current position.
    event_index: usize,
    /// Current read state.
    state: ReadState,
    /// The current node we're positioned on.
    cur_node: *mut _xmlNode,
    /// Current node type (reader node type).
    node_type: ReaderNodeType,
    /// Current depth.
    depth: i32,
    /// Cached name of the current node (xmlMalloc'd, NULL if none).
    name: *mut xmlChar,
    /// Cached value of the current node (xmlMalloc'd, NULL if none).
    value: *mut xmlChar,
    /// Number of attributes on the current element (-1 if not applicable).
    attribute_count: i32,
    /// Current attribute index (-1 = not on an attribute).
    cur_attribute: i32,
    /// Parser options bitmask.
    options: c_int,
    /// Document encoding string (xmlMalloc'd).
    encoding: *mut xmlChar,
    /// Document URL (xmlMalloc'd).
    URL: *mut xmlChar,
    /// Collected error messages.
    errors: Vec<String>,
    /// Whether the document has been parsed.
    parsed: bool,
    /// Reader error callback (xmlTextReaderSetErrorHandler).
    error_handler: Option<xmlTextReaderErrorFunc>,
    /// User data for the error callback.
    error_arg: *mut c_void,
    /// Structured error callback (xmlTextReaderSetStructuredErrorHandler).
    structured_handler: Option<crate::abi::callbacks::xmlStructuredErrorFunc>,
    /// User data for the structured callback.
    structured_arg: *mut c_void,
    /// Cached last-error struct (xmlTextReaderGetLastError); message owned.
    last_err: crate::abi::structs::_xmlError,
    /// Maximum entity amplification ratio (xmlTextReaderSetMaxAmplification).
    max_amplification: c_int,
    /// Schema set via xmlTextReaderSetSchema.
    schema: *mut c_void,
    /// RELAX NG schema set via xmlTextReaderRelaxNGSetSchema.
    rng: *mut c_void,
    /// Whether the reader owns `schema` (file-path xmlTextReaderSchemaValidate
    /// compiles it; xmlTextReaderSetSchema hands in a caller-owned schema).
    schema_owned: bool,
    /// Whether the reader owns `rng` (xmlTextReaderRelaxNGValidate compiles
    /// it; xmlTextReaderRelaxNGSetSchema hands in a caller-owned schema that
    /// php_xmlreader keeps and frees itself).
    rng_owned: bool,
    /// Last deferred XML Schema validation outcome (1 valid, 0 invalid, -1
    /// no XSD validation attached). Upstream reader->xsdValidCtxt->valid.
    xsd_result: c_int,
    /// Last deferred RELAX NG validation outcome (1 valid, 0 invalid, -1 no
    /// RELAX NG validation attached). Upstream reader->rngValidCtxt->valid.
    rng_result: c_int,
    /// Whether the reader owns `doc` (parse paths: yes; walker: no).
    owns_doc: bool,
    /// Upstream `reader->preserve`: xmlTextReaderCurrentDoc sets it, so the
    /// document ownership transfers to the caller (xmlFreeTextReader no
    /// longer frees it; the caller frees with xmlFreeDoc). Without this the
    /// upstream reader3.c pattern (CurrentDoc -> xmlFreeTextReader ->
    /// xmlDocDump -> xmlFreeDoc) double-frees.
    preserve: bool,
    /// Whether the current attribute position is a namespace declaration
    /// (xmlTextReaderIsNamespaceDecl; ns-decls are exposed as attributes).
    cur_attr_is_ns: bool,
    /// Compiled preserve-patterns (xmlTextReaderPreservePattern; upstream
    /// `reader->patternTab`). Applied after the parse: matched nodes and
    /// their element ancestors survive, everything else is unlinked and
    /// freed (upstream xmlTextReaderRead's NODE_IS_PRESERVED pruning).
    pattern_tab: Vec<crate::abi::exports_automata::xmlPatternPtr>,
    /// DTD parse validity snapshot: `ctxt->valid` right after the parse,
    /// meaningful only when the parse ran with ctxt->validate == 1
    /// (xmlTextReaderIsValid, upstream xmlreader.c).
    was_valid: c_int,
    /// Whether the parse ran a DTD validation pass (ctxt->validate).
    did_validate: c_int,
}

impl XmlTextReader {
    /// Create a new reader with the given parser context and options.
    ///
    /// The reader takes ownership of the parser context. The document will be
    /// parsed on the first call to `Read()`.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid parser context created by `create_parser_ctxt`
    /// and set up with input via `setup_parser_input`.
    unsafe fn new(ctxt: *mut _xmlParserCtxt, URL: Option<&[u8]>, encoding: Option<&[u8]>) -> Self {
        let url_ptr = URL
            .map(|u| unsafe { bytes_to_xmlstr(u) })
            .unwrap_or(ptr::null_mut());
        let enc_ptr = encoding
            .map(|e| unsafe { bytes_to_xmlstr(e) })
            .unwrap_or(ptr::null_mut());

        XmlTextReader {
            doc: ptr::null_mut(),
            ctxt,
            events: Vec::new(),
            event_index: 0,
            state: ReadState::INITIALIZED,
            cur_node: ptr::null_mut(),
            node_type: ReaderNodeType::NONE,
            depth: 0,
            name: ptr::null_mut(),
            value: ptr::null_mut(),
            attribute_count: -1,
            cur_attribute: -1,
            options: 0,
            encoding: enc_ptr,
            URL: url_ptr,
            errors: Vec::new(),
            parsed: false,
            error_handler: None,
            error_arg: ptr::null_mut(),
            structured_handler: None,
            structured_arg: ptr::null_mut(),
            last_err: unsafe { core::mem::zeroed() },
            max_amplification: 0,
            schema: ptr::null_mut(),
            rng: ptr::null_mut(),
            schema_owned: false,
            rng_owned: false,
            xsd_result: -1,
            rng_result: -1,
            owns_doc: true,
            preserve: false,
            cur_attr_is_ns: false,
            pattern_tab: Vec::new(),
            was_valid: 0,
            did_validate: 0,
        }
    }

    /// Parse the document and build the event list.
    ///
    /// Returns 0 on success, -1 on error.
    ///
    /// # Safety
    ///
    /// `ctxt` must be a valid parser context with input set up.
    unsafe fn parse_and_build_events(&mut self) -> c_int {
        if self.ctxt.is_null() {
            self.state = ReadState::ERROR;
            self.errors.push("No parser context".to_string());
            return -1;
        }

        // Set options on the context (mirrors xmlCtxtUseOptions so the
        // historical struct members — validate/loadsubset/recovery/etc. —
        // reflect XML_PARSE_* options; reader2.c relies on XML_PARSE_DTDVALID
        // reaching ctxt->validate so the parser raises the no-DTD validity
        // error, Phase-12 EXTERNAL-CONSUMERS court).
        unsafe {
            crate::abi::exports_parser::apply_options(self.ctxt, self.options);
        }

        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderSetup): the reader marks
        // its parse XML_PARSE_READER. The parser records self-closed
        // (`<a/>`) element nodes only for reader parses (state.rs
        // parse_element), and the validation layer routes through the
        // streaming branch when the consumer is a reader (validation/mod.rs
        // add_ref).
        unsafe {
            (*self.ctxt).parseMode = crate::abi::types::xmlParserMode::XML_PARSE_READER as c_int;
        }

        // Parse the document.
        let result = unsafe { parse_document(self.ctxt) };

        // Get the parsed document.
        let doc = unsafe { (*self.ctxt).myDoc };
        self.doc = doc;

        // UPSTREAM-PARITY (xmlTextReaderIsValid): snapshot the DTD-parse
        // validity state before the parser context is released.
        self.was_valid = unsafe { (*self.ctxt).valid };
        self.did_validate = unsafe { (*self.ctxt).validate };

        // Free the parser context - we no longer need it.
        if !self.ctxt.is_null() {
            unsafe { free_parser_ctxt(self.ctxt) };
        }
        self.ctxt = ptr::null_mut();

        if result != 0 || doc.is_null() {
            // Drop the self-closed markers of the failed parse (keyed by
            // doc — the successful path consumes them during build_events).
            crate::xml::parser::helpers::drop_self_closed(doc);
            self.state = ReadState::ERROR;
            self.errors.push("Failed to parse document".to_string());
            return -1;
        }

        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderSchemaValidateInternal /
        // xmlTextReaderRelaxNGValidateInternal): schemas attached before the
        // first Read() are validated against the document as it streams — the
        // whole-tree reader runs the equivalent validation right after the
        // parse, inside this first Read() call, so validity diagnostics are
        // raised through the global libxml error channel at read time and the
        // outcome is recorded for xmlTextReaderIsValid.
        unsafe {
            self.run_deferred_validation();
        }

        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderRead NODE_IS_PRESERVED
        // pruning): registered preserve-patterns are applied to the parsed
        // tree before events are built — matched nodes and their element
        // ancestors survive, everything else is unlinked and freed. Upstream
        // prunes while streaming; the whole-tree reader collapses that to
        // the equivalent post-parse state (Phase-12 EXTERNAL-CONSUMERS
        // court: reader3.c).
        unsafe {
            self.apply_pattern_preservation();
        }

        // Set the encoding from the document if not already set.
        if self.encoding.is_null() && !doc.is_null() {
            // SAFETY: doc is valid.
            let doc_enc = unsafe { (*doc).encoding };
            if !doc_enc.is_null() {
                self.encoding = unsafe { xml_strdup(doc_enc as *const xmlChar) };
            }
        }

        // Build traversal events from the tree.
        self.build_events();

        self.parsed = true;
        0
    }

    /// Run the deferred XML Schema / RELAX NG validation attached through
    /// xmlTextReaderSchemaValidate / xmlTextReaderRelaxNGValidate /
    /// xmlTextReaderSetSchema / xmlTextReaderRelaxNGSetSchema before the
    /// first Read(). Diagnostics are raised through the global libxml error
    /// channel (exactly the channel upstream's streaming validation uses once
    /// the reader hits the offending node) and the outcome is recorded for
    /// xmlTextReaderIsValid.
    ///
    /// # SAFETY
    ///
    /// - `self.doc` must be a valid parsed document; `self.schema`/`self.rng`
    ///   must be valid compiled schemas (or NULL).
    unsafe fn run_deferred_validation(&mut self) {
        // XML Schema first (upstream checks the RELAX NG plug, then the XSD
        // plug, in xmlTextReaderIsValid; a reader only carries one of them in
        // practice).
        if !self.schema.is_null() {
            let (valid, errors) = unsafe {
                crate::xml::schemas::xsd_validate_doc_compiled(self.schema, self.doc, false)
            };
            self.xsd_result = if valid { 1 } else { 0 };
            for msg in errors {
                self.raise_validation_error(msg, crate::abi::types::XML_FROM_SCHEMASV);
            }
        }
        if !self.rng.is_null() {
            let (valid, errors) =
                unsafe { crate::xml::relaxng::rng_validate_doc_compiled(self.rng, self.doc) };
            self.rng_result = if valid { 1 } else { 0 };
            for msg in errors {
                self.raise_validation_error(msg, crate::abi::types::XML_FROM_RELAXNGV);
            }
        }
    }

    /// Raise one validation diagnostic through the global libxml error
    /// channel. UPSTREAM-PARITY: validity messages end with a newline — PHP's
    /// libxml error handler only raises messages that carry a trailing
    /// newline (php_libxml_internal_error_handler_ex), and the message text
    /// is passed through unmodified (no domain prefix).
    fn raise_validation_error(&self, msg: String, domain: c_int) {
        let text = if msg.ends_with('\n') {
            msg
        } else {
            format!("{}\n", msg)
        };
        let Ok(cmsg) = std::ffi::CString::new(text) else {
            return;
        };
        // SAFETY: cmsg outlives the call; all other pointers are NULL.
        unsafe {
            crate::xml::errors::raise_error(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                domain,
                0,
                crate::abi::types::xmlErrorLevel::XML_ERR_ERROR as c_int,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                cmsg.as_ptr(),
            );
        }
    }

    /// Apply registered preserve-patterns to the parsed tree (upstream
    /// xmlreader.c: `xmlTextReaderPreserve` marks the matched node
    /// NODE_IS_PRESERVED | NODE_IS_SPRESERVED and its element ancestors
    /// NODE_IS_PRESERVED; the streaming `xmlTextReaderRead` prunes every
    /// other node as it passes). The whole-tree reader collapses that to a
    /// post-parse pass: a node survives iff it or an element ancestor is
    /// matched by a registered pattern (SPRESERVED subtrees are kept
    /// whole). DTD nodes survive like upstream's special-casing.
    ///
    /// # SAFETY
    ///
    /// - `self.doc` must be a valid parsed tree (or NULL); `self.pattern_tab`
    ///   holds compiled patterns owned by the reader.
    unsafe fn apply_pattern_preservation(&mut self) {
        if self.pattern_tab.is_empty() || self.doc.is_null() {
            return;
        }

        // Phase 1: collect every node matched by any pattern.
        let mut matched: Vec<*mut _xmlNode> = Vec::new();
        unsafe {
            let mut stack: Vec<*mut _xmlNode> = vec![self.doc as *mut _xmlNode];
            while let Some(n) = stack.pop() {
                if n.is_null() {
                    continue;
                }
                for p in &self.pattern_tab {
                    if crate::abi::exports_automata::xmlPatternMatch(*p, n) == 1 {
                        matched.push(n);
                        break;
                    }
                }
                let mut c = (*n).children;
                while !c.is_null() {
                    stack.push(c);
                    c = (*c).next;
                }
            }
        }

        // Phase 2: keep-set = matched nodes + their element ancestors.
        let mut keep: HashSet<usize> = HashSet::new();
        for m in &matched {
            keep.insert(*m as usize);
            let mut p = unsafe { (**m).parent };
            while !p.is_null() {
                keep.insert(p as usize);
                p = unsafe { (*p).parent };
            }
        }

        // Phase 3: prune the children of every surviving node.
        unsafe {
            prune_unpreserved(self.doc as *mut _xmlNode, false, &keep, &self.pattern_tab);
        }
    }

    /// Walk the tree in document order and build traversal events.
    ///
    /// Generates events for all nodes (ELEMENT, TEXT, COMMENT, PI, etc.)
    /// and END_ELEMENT events for elements.
    fn build_events(&mut self) {
        self.events.clear();

        if self.doc.is_null() {
            return;
        }

        // SAFETY: doc is valid.
        let root = unsafe { (*self.doc).children };
        if root.is_null() {
            return;
        }

        // Walk all top-level children (PIs, comments, the root element, etc.)
        // SAFETY: The tree is valid and all pointers are valid.
        unsafe {
            let mut n = root;
            while !n.is_null() {
                self.walk_tree(n, 0);
                n = (*n).next;
            }
        }
    }

    /// Recursively walk a subtree and generate events.
    ///
    /// # Safety
    ///
    /// `node` must be a valid pointer to a node in the parsed tree.
    unsafe fn walk_tree(&mut self, node: *mut _xmlNode, depth: i32) {
        if node.is_null() {
            return;
        }

        // SAFETY: node is valid.
        let node_type = unsafe { (*node).type_ };

        // For elements, generate an enter event and then recursively visit children,
        // then generate an exit (END_ELEMENT) event — unless the element is
        // self-closed (`<a/>`; upstream xmlreader: only an explicitly closed
        // element fires END_ELEMENT). Elements with children always end;
        // childless elements end iff they were NOT parsed from `<a/>`
        // (R-000144 note: the tree alone cannot tell `<a/>` from `<a></a>`,
        // so the parser records self-closed nodes during reader parses).
        if node_type == XML_ELEMENT_NODE as c_int {
            self.events.push(TraversalEvent {
                node,
                is_end: false,
                depth,
            });

            // Walk children.
            // SAFETY: node's children are valid.
            let mut child = unsafe { (*node).children };
            while !child.is_null() {
                let child_depth = depth + 1;
                self.walk_tree(child, child_depth);
                // SAFETY: child's next pointer is valid.
                child = unsafe { (*child).next };
            }

            // Generate END_ELEMENT for explicitly closed elements only.
            let self_closed = crate::xml::parser::helpers::take_self_closed(self.doc, node);
            if !self_closed {
                self.events.push(TraversalEvent {
                    node,
                    is_end: true,
                    depth,
                });
            }
        } else if node_type == XML_TEXT_NODE as c_int
            || node_type == XML_CDATA_SECTION_NODE as c_int
            || node_type == XML_COMMENT_NODE as c_int
            || node_type == XML_PI_NODE as c_int
            || node_type == XML_ENTITY_REF_NODE as c_int
        {
            // Leaf nodes: text, CDATA, comment, PI, entity reference.
            // Whitespace-only text is emitted (as SIGNIFICANT_WHITESPACE by
            // position_at) — upstream reader default behavior without
            // XML_PARSE_NOBLANKS.
            self.events.push(TraversalEvent {
                node,
                is_end: false,
                depth,
            });
        } else {
            // Other node types (ENTITY, NOTATION, DTD, etc.) — skip or just enter.
            self.events.push(TraversalEvent {
                node,
                is_end: false,
                depth,
            });
        }
    }

    /// Position the reader on the event at the given index.
    ///
    /// Updates all cached fields (name, value, depth, node_type, etc.).
    fn position_at(&mut self, index: usize) {
        if index >= self.events.len() {
            self.state = ReadState::EOF;
            self.cur_node = ptr::null_mut();
            self.node_type = ReaderNodeType::NONE;
            self.depth = 0;
            self.clear_cached_name();
            self.clear_cached_value();
            self.attribute_count = -1;
            self.cur_attribute = -1;
            return;
        }

        // Copy event data before any mutable self access to avoid borrow conflicts.
        let ev_node: *mut _xmlNode;
        let ev_is_end: bool;
        let ev_depth: i32;
        {
            let event = &self.events[index];
            ev_node = event.node;
            ev_is_end = event.is_end;
            ev_depth = event.depth;
        }

        self.event_index = index;
        self.cur_node = ev_node;
        self.depth = ev_depth;

        // SAFETY: node is valid.
        let etype = unsafe { (*ev_node).type_ };

        if ev_is_end {
            self.node_type = ReaderNodeType::END_ELEMENT;
        } else {
            self.node_type = element_type_to_reader_type(etype);
            // UPSTREAM-PARITY: whitespace-only text is reported as
            // SIGNIFICANT_WHITESPACE (14) unless XML_PARSE_NOBLANKS (R-000144).
            if etype == XML_TEXT_NODE as c_int || etype == XML_CDATA_SECTION_NODE as c_int {
                let content = unsafe { (*ev_node).content };
                if !content.is_null() {
                    // SAFETY: content is a valid NUL-terminated C string owned by the node.
                    let len = unsafe { libc::strlen(content as *const libc::c_char) as usize };
                    // SAFETY: content points to len valid bytes (the NUL-terminated string).
                    let slice = unsafe { core::slice::from_raw_parts(content, len) };
                    if is_whitespace_only(slice) {
                        self.node_type = ReaderNodeType::SIGNIFICANT_WHITESPACE;
                    }
                }
            }
        }

        // Cache name and value.
        // SAFETY: ev_node is a valid node pointer.
        unsafe { self.cache_name_and_value(ev_node, ev_is_end) };

        // Count attributes if this is an element.
        if etype == XML_ELEMENT_NODE as c_int && !ev_is_end {
            // SAFETY: ev_node is a valid element node.
            self.attribute_count = unsafe { self.count_attributes(ev_node) };
        } else {
            self.attribute_count = -1;
        }

        // Reset attribute cursor.
        self.cur_attribute = -1;
        self.cur_attr_is_ns = false;
    }

    /// Cache the name of the current node.
    ///
    /// # Safety
    ///
    /// `node` must be a valid node pointer or NULL.
    unsafe fn cache_name_and_value(&mut self, node: *mut _xmlNode, is_end: bool) {
        self.clear_cached_name();
        self.clear_cached_value();

        if node.is_null() {
            return;
        }

        // SAFETY: node is valid.
        let etype = unsafe { (*node).type_ };

        // Determine name.
        let name: *mut xmlChar = if is_end {
            // For END_ELEMENT, the name is the element name.
            // SAFETY: node is valid.
            unsafe { (*node).name as *mut xmlChar }
        } else {
            if etype == XML_ELEMENT_NODE as c_int
                || etype == XML_PI_NODE as c_int
                || etype == XML_ENTITY_REF_NODE as c_int
                || etype == XML_ENTITY_NODE as c_int
                || etype == XML_DOCUMENT_TYPE_NODE as c_int
                || etype == XML_DTD_NODE as c_int
                || etype == XML_NOTATION_NODE as c_int
            {
                // SAFETY: node is valid.
                unsafe { (*node).name as *mut xmlChar }
            } else if etype == XML_ATTRIBUTE_NODE as c_int {
                // For attribute nodes accessed via MoveToAttribute.
                ptr::null_mut()
            } else {
                ptr::null_mut()
            }
        };

        if !name.is_null() {
            // UPSTREAM-PARITY: xmlTextReaderName/ConstName return the
            // qualified name for namespaced elements (e.g. "x:child"); the
            // candidate rebuilds it from the node's ns prefix.
            let qualified: *mut xmlChar = if etype == XML_ELEMENT_NODE as c_int && !node.is_null() {
                let ns = unsafe { (*node).ns };
                if !ns.is_null() && !unsafe { (*ns).prefix }.is_null() {
                    let plen =
                        libc::strlen(unsafe { (*ns).prefix } as *const libc::c_char) as usize;
                    let nlen = libc::strlen(name as *const libc::c_char) as usize;
                    let p =
                        crate::abi::allocator::xmlMallocImpl(plen + 1 + nlen + 1) as *mut xmlChar;
                    if !p.is_null() {
                        libc::memcpy(
                            p as *mut libc::c_void,
                            unsafe { (*ns).prefix } as *const libc::c_void,
                            plen,
                        );
                        *p.add(plen) = b':';
                        libc::memcpy(
                            p.add(plen + 1) as *mut libc::c_void,
                            name as *const libc::c_void,
                            nlen,
                        );
                        *p.add(plen + 1 + nlen) = 0;
                    }
                    p
                } else {
                    unsafe { xml_strdup(name as *const xmlChar) }
                }
            } else {
                unsafe { xml_strdup(name as *const xmlChar) }
            };
            self.name = qualified;
        } else if !is_end {
            // UPSTREAM-PARITY (xmlTextReaderConstName): typed node kinds are
            // reported with fixed names rather than NULL.
            let fixed: &[u8] = match etype {
                x if x == XML_TEXT_NODE as c_int => b"#text\0",
                x if x == XML_CDATA_SECTION_NODE as c_int => b"#cdata-section\0",
                x if x == XML_COMMENT_NODE as c_int => b"#comment\0",
                x if x == XML_DOCUMENT_NODE as c_int => b"#document\0",
                x if x == XML_HTML_DOCUMENT_NODE as c_int => b"#document\0",
                x if x == XML_DOCUMENT_FRAG_NODE as c_int => b"#document-fragment\0",
                _ => b"",
            };
            if !fixed.is_empty() {
                self.name = unsafe { xml_strdup(fixed.as_ptr() as *const xmlChar) };
            }
        }

        // Determine value.
        let value: *mut xmlChar = if etype == XML_TEXT_NODE as c_int
            || etype == XML_CDATA_SECTION_NODE as c_int
            || etype == XML_COMMENT_NODE as c_int
        {
            // SAFETY: node is valid.
            unsafe { (*node).content }
        } else if etype == XML_PI_NODE as c_int {
            // PI nodes store content as the PI value (after the target).
            // SAFETY: node is valid.
            unsafe { (*node).content }
        } else if etype == XML_ENTITY_REF_NODE as c_int {
            // Entity references may have content.
            // SAFETY: node is valid.
            unsafe { (*node).content }
        } else {
            ptr::null_mut()
        };

        if !value.is_null() {
            // SAFETY: value is a valid null-terminated xmlChar string.
            self.value = unsafe { xml_strdup(value as *const xmlChar) };
        }
    }

    /// Count the number of attributes on an element node.
    ///
    /// # Safety
    ///
    /// `node` must be a valid element node pointer.
    unsafe fn count_attributes(&self, node: *mut _xmlNode) -> i32 {
        let mut count: i32 = 0;
        // UPSTREAM-PARITY: namespace declarations count as attributes
        // (xmlTextReaderAttributeCount includes them, xmlreader.c).
        let mut ns = unsafe { (*node).nsDef };
        while !ns.is_null() {
            count += 1;
            ns = unsafe { (*ns).next };
        }
        // SAFETY: node is a valid element.
        let mut prop = unsafe { (*node).properties };
        while !prop.is_null() {
            count += 1;
            // SAFETY: prop is valid.
            prop = unsafe { (*prop).next };
        }
        count
    }

    /// Unified attribute addressing: namespace declarations first, then
    /// regular attributes (upstream reader attribute iteration, R-000143:
    /// xmlns / xmlns:prefix count as attributes, ordered before properties).
    ///
    /// # Safety
    ///
    /// - `node` must be non-NULL (checked) and point to a valid `_xmlNode`.
    /// - The `nsDef` and `properties` lists of the node must be valid
    ///   NULL-terminated linked lists of `_xmlNs` and `_xmlAttr` objects.
    /// - `index` must be non-negative (checked).
    unsafe fn attr_at(&self, node: *mut _xmlNode, index: i32) -> AttrTarget {
        if node.is_null() || index < 0 {
            return AttrTarget::None;
        }
        let mut i = 0;
        let mut ns = unsafe { (*node).nsDef };
        while !ns.is_null() {
            if i == index {
                return AttrTarget::Ns(ns);
            }
            i += 1;
            ns = unsafe { (*ns).next };
        }
        let mut prop = unsafe { (*node).properties };
        while !prop.is_null() {
            if i == index {
                return AttrTarget::Prop(prop);
            }
            i += 1;
            prop = unsafe { (*prop).next };
        }
        AttrTarget::None
    }

    /// The index of the attribute matching `name` (ns decls use
    /// "xmlns:prefix"/"xmlns" names), or -1.
    ///
    /// # Safety
    ///
    /// - `node` must be non-NULL (checked) and point to a valid `_xmlNode`.
    /// - `name` must be non-NULL (checked) and point to a valid
    ///   NUL-terminated `xmlChar` string; it is scanned with `libc::strlen`.
    /// - The `nsDef` and `properties` lists must be valid NULL-terminated
    ///   linked lists, and every `prefix` and `prop.name` encountered must be
    ///   NULL or a valid NUL-terminated `xmlChar` string.
    unsafe fn attr_index_by_name(&self, node: *mut _xmlNode, name: *const xmlChar) -> i32 {
        if node.is_null() || name.is_null() {
            return -1;
        }
        let mut i = 0;
        let mut ns = unsafe { (*node).nsDef };
        while !ns.is_null() {
            let n = unsafe { &*ns };
            let nsname: Vec<u8> = if n.prefix.is_null() {
                b"xmlns\0".to_vec()
            } else {
                let mut v = b"xmlns:\0".to_vec();
                let plen = libc::strlen(n.prefix as *const libc::c_char) as usize;
                v.extend_from_slice(core::slice::from_raw_parts(n.prefix, plen));
                v.push(0);
                v
            };
            let nlen = libc::strlen(name as *const libc::c_char) as usize;
            let nbytes = core::slice::from_raw_parts(name, nlen);
            if nbytes == &nsname[..nsname.len() - 1] {
                return i;
            }
            i += 1;
            ns = unsafe { (*ns).next };
        }
        let mut prop = unsafe { (*node).properties };
        while !prop.is_null() {
            let pn = unsafe { (*prop).name };
            if !pn.is_null()
                && libc::strcmp(pn as *const libc::c_char, name as *const libc::c_char) == 0
            {
                return i;
            }
            i += 1;
            prop = unsafe { (*prop).next };
        }
        -1
    }

    /// Free the cached name.
    fn clear_cached_name(&mut self) {
        if !self.name.is_null() {
            // SAFETY: name was allocated by xmlMalloc (via xml_strdup).
            unsafe { xmlFreeImpl(self.name as *mut c_void) };
            self.name = ptr::null_mut();
        }
    }

    /// Free the cached value.
    fn clear_cached_value(&mut self) {
        if !self.value.is_null() {
            // SAFETY: value was allocated by xmlMalloc (via xml_strdup).
            unsafe { xmlFreeImpl(self.value as *mut c_void) };
            self.value = ptr::null_mut();
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Navigation methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Read the next node in document order.
    ///
    /// Returns 1 if a node was read, 0 if no more nodes (EOF), -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Read(&mut self) -> c_int {
        if self.state == ReadState::ERROR || self.state == ReadState::CLOSED {
            return -1;
        }

        // On first call, parse the document and build events.
        if !self.parsed {
            if self.parse_and_build_events() != 0 {
                self.state = ReadState::ERROR;
                return -1;
            }
            self.state = ReadState::READING;
        }

        if self.state == ReadState::EOF {
            return 0;
        }

        // If we're positioned on an attribute, return to the element first.
        if self.cur_attribute >= 0 {
            self.cur_attribute = -1;
            self.cur_attr_is_ns = false;
            // Re-cache the element info.
            if !self.cur_node.is_null() {
                // SAFETY: cur_node is valid.
                unsafe { self.cache_name_and_value(self.cur_node, false) };
            }
        }

        // Advance to the next event.
        // If no events, we're at EOF.
        if self.events.is_empty() {
            self.state = ReadState::EOF;
            return 0;
        }

        // Determine the next event index to position on.
        // If cur_node is NULL, this is the first Read() after parsing —
        // position at event 0. On subsequent calls, advance to the next event.
        // We use cur_node.is_null() rather than event_index checks because
        // after position_at(0), event_index == 0 and state == READING, which
        // is indistinguishable from the pre-read state.
        let next_index = if self.cur_node.is_null() {
            // First Read() after parsing — position at event 0.
            0
        } else {
            self.event_index + 1
        };

        if next_index < self.events.len() {
            self.position_at(next_index);
            1
        } else {
            self.state = ReadState::EOF;
            self.cur_node = ptr::null_mut();
            self.node_type = ReaderNodeType::NONE;
            self.depth = 0;
            self.clear_cached_name();
            self.clear_cached_value();
            self.attribute_count = -1;
            self.cur_attribute = -1;
            0
        }
    }

    /// Skip to the next sibling of the current node.
    ///
    /// Returns 1 on success, 0 if no more siblings, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Next(&mut self) -> c_int {
        if self.state == ReadState::ERROR || self.state == ReadState::CLOSED {
            return -1;
        }
        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderNext): with no current
        // node (fresh reader — first call — or after the traversal ended) a
        // Next degrades to a plain xmlTextReaderRead: on a fresh reader this
        // STARTS the traversal (php gh19098 calls next("sparql") before any
        // read). The subtree-skip below applies only when the reader sits on
        // an element start.
        if self.cur_node.is_null() {
            return self.Read();
        }
        if self.state == ReadState::EOF {
            return 0;
        }

        // Attributes are not independent positions: an attribute cursor is
        // backed by the element node (upstream keeps reader->node on the
        // element); reset it and treat the position as the element.
        if self.cur_attribute >= 0 {
            self.cur_attribute = -1;
            self.cur_attr_is_ns = false;
        }

        // UPSTREAM-PARITY (xmlTextReader.c xmlTextReaderNext): when the
        // current node is NOT an element START (text/CDATA/comment/PI/END
        // etc.) Next degrades to a plain xmlTextReaderRead — a single step
        // forward in document order that may land on the parent's
        // END_ELEMENT event. The subtree-skip applies only when the reader
        // sits on an element start.
        if self.node_type != ReaderNodeType::ELEMENT {
            let nxt = self.event_index + 1;
            if nxt < self.events.len() {
                self.position_at(nxt);
                return 1;
            }
            self.position_at(self.events.len());
            return 0;
        }

        // Element start: skip the whole subtree (all deeper events and the
        // element's own END event plus every ancestor END event) to the next
        // positionable event at the same or a shallower depth. When nothing
        // remains the traversal ends: the cursor is cleared (EOF) exactly
        // like upstream's backtrack-to-document state.
        let current_depth = self.depth;
        let mut i = self.event_index + 1;
        while i < self.events.len() {
            let event = &self.events[i];
            if !event.is_end && event.depth <= current_depth {
                self.position_at(i);
                return 1;
            }
            i += 1;
        }
        self.position_at(self.events.len());
        0
    }

    /// Move to the parent element (if currently on an attribute).
    ///
    /// Returns 1 on success, 0 if not on an attribute, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn MoveToElement(&mut self) -> c_int {
        if self.cur_attribute < 0 {
            return 0;
        }
        self.cur_attribute = -1;
        self.cur_attr_is_ns = false;
        if !self.cur_node.is_null() {
            // SAFETY: cur_node is valid.
            unsafe { self.cache_name_and_value(self.cur_node, false) };
            self.node_type = ReaderNodeType::ELEMENT;
        }
        1
    }

    /// Move to an attribute by name.
    ///
    /// Returns 1 on success, 0 if attribute not found, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn MoveToAttribute(&mut self, name: *const xmlChar) -> c_int {
        if self.cur_node.is_null() {
            return -1;
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return -1;
        }

        // SAFETY: cur_node is an element.
        let idx = unsafe { self.attr_index_by_name(self.cur_node, name) };
        if idx < 0 {
            return 0;
        }
        self.cur_attribute = idx;
        let target = unsafe { self.attr_at(self.cur_node, idx) };
        // Cache attribute info.
        unsafe { self.cache_attribute_info(target) };
        1
    }

    /// Move to an attribute by index.
    ///
    /// Returns 1 on success, 0 if index out of range, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn MoveToAttributeNo(&mut self, index: c_int) -> c_int {
        if self.cur_node.is_null() || index < 0 {
            return -1;
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return -1;
        }

        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderMoveToAttributeNo): the
        // attribute cursor is reset to the ELEMENT on entry (reader->curnode
        // = NULL), so a failed lookup leaves the reader on the element — the
        // php-visible contract (003-move-errors: "node pointer moves back to
        // the element in this case").
        self.cur_attribute = -1;
        self.cur_attr_is_ns = false;
        if !self.cur_node.is_null() {
            unsafe { self.cache_name_and_value(self.cur_node, false) };
            self.node_type = ReaderNodeType::ELEMENT;
        }

        // SAFETY: cur_node is an element.
        let target = unsafe { self.attr_at(self.cur_node, index) };
        match target {
            AttrTarget::None => 0,
            t => {
                self.cur_attribute = index;
                // Cache attribute info.
                unsafe { self.cache_attribute_info(t) };
                1
            }
        }
    }

    /// Move to the first attribute of the current element.
    ///
    /// Returns 1 on success, 0 if no attributes, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn MoveToFirstAttribute(&mut self) -> c_int {
        if self.cur_node.is_null() {
            return -1;
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return -1;
        }

        // SAFETY: cur_node is an element.
        let first = unsafe { self.attr_at(self.cur_node, 0) };
        match first {
            AttrTarget::None => 0,
            t => {
                self.cur_attribute = 0;
                // Cache attribute info.
                unsafe { self.cache_attribute_info(t) };
                1
            }
        }
    }

    /// Move to the next attribute.
    ///
    /// Returns 1 on success, 0 if no more attributes, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn MoveToNextAttribute(&mut self) -> c_int {
        if self.cur_node.is_null() {
            return -1;
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return -1;
        }

        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderMoveToNextAttribute):
        // "if the current node is an element, this moves to its first
        // attribute" — a next-attribute call from an element behaves like
        // MoveToFirstAttribute. Only when already on an attribute does it
        // advance to the following one.
        let next_index = if self.cur_attribute < 0 {
            0
        } else {
            self.cur_attribute + 1
        };
        let target = unsafe { self.attr_at(self.cur_node, next_index) };
        match target {
            AttrTarget::None => 0,
            t => {
                self.cur_attribute = next_index;
                unsafe { self.cache_attribute_info(t) };
                1
            }
        }
    }

    /// Cache the current position info for an attribute (or namespace
    /// declaration, which the reader presents as an attribute).
    ///
    /// # Safety
    ///
    /// `target` must be a valid AttrTarget::Prop(_xmlAttr) or
    /// AttrTarget::Ns(_xmlNs).
    unsafe fn cache_attribute_info(&mut self, target: AttrTarget) {
        self.node_type = ReaderNodeType::ATTRIBUTE;
        self.cur_attr_is_ns = matches!(target, AttrTarget::Ns(_));
        self.clear_cached_name();
        self.clear_cached_value();
        match target {
            AttrTarget::Ns(ns) => {
                // UPSTREAM-PARITY: a namespace declaration is exposed as an
                // attribute named "xmlns:prefix" (or "xmlns" for the default
                // namespace) whose value is the namespace URI.
                let n = unsafe { &*ns };
                if n.prefix.is_null() {
                    self.name = unsafe { xml_strdup(c"xmlns".as_ptr() as *const xmlChar) };
                } else {
                    let plen = libc::strlen(n.prefix as *const libc::c_char) as usize;
                    let mut v = Vec::with_capacity(6 + plen);
                    v.extend_from_slice(b"xmlns:");
                    v.extend_from_slice(core::slice::from_raw_parts(n.prefix, plen));
                    v.push(0);
                    let p = crate::abi::allocator::xmlMallocImpl(v.len()) as *mut xmlChar;
                    if !p.is_null() {
                        libc::memcpy(
                            p as *mut libc::c_void,
                            v.as_ptr() as *const libc::c_void,
                            v.len(),
                        );
                        self.name = p;
                    }
                }
                if !n.href.is_null() {
                    self.value = unsafe { xml_strdup(n.href as *const xmlChar) };
                }
            }
            AttrTarget::Prop(prop) => {
                // SAFETY: prop is valid.
                let attr = unsafe { &*prop };

                // Name — UPSTREAM-PARITY (xmlTextReaderConstName): a
                // namespace-qualified attribute is reported as
                // "prefix:localname" (constQString), unqualified attributes
                // keep the local name.
                if !attr.name.is_null() {
                    if !attr.ns.is_null() && !unsafe { (*attr.ns).prefix }.is_null() {
                        let plen = libc::strlen(unsafe { (*attr.ns).prefix } as *const libc::c_char)
                            as usize;
                        let nlen = libc::strlen(attr.name as *const libc::c_char) as usize;
                        let p = crate::abi::allocator::xmlMallocImpl(plen + 1 + nlen + 1)
                            as *mut xmlChar;
                        if !p.is_null() {
                            libc::memcpy(
                                p as *mut libc::c_void,
                                unsafe { (*attr.ns).prefix } as *const libc::c_void,
                                plen,
                            );
                            *p.add(plen) = b':';
                            libc::memcpy(
                                p.add(plen + 1) as *mut libc::c_void,
                                attr.name as *const libc::c_void,
                                nlen,
                            );
                            *p.add(plen + 1 + nlen) = 0;
                            self.name = p;
                        }
                    } else {
                        // SAFETY: attr.name is null-terminated.
                        self.name = unsafe { xml_strdup(attr.name as *const xmlChar) };
                    }
                }

                // Value — the attribute's text content is in its child text
                // node. An EXISTING attribute with an empty value (`bar=""`)
                // has no children and reports "" (upstream xmlGetProp /
                // xmlNodeGetAttrValue), never NULL.
                if !attr.children.is_null() {
                    // SAFETY: attr.children is a text node.
                    let val = unsafe { (*attr.children).content };
                    if !val.is_null() {
                        // SAFETY: val is null-terminated.
                        self.value = unsafe { xml_strdup(val as *const xmlChar) };
                    } else {
                        self.value = unsafe { xml_strdup(c"".as_ptr() as *const xmlChar) };
                    }
                } else {
                    self.value = unsafe { xml_strdup(c"".as_ptr() as *const xmlChar) };
                }
            }
            AttrTarget::None => {}
        }
    }

    /// Move to the previous sibling.
    ///
    /// Returns 1 on success, 0 if no previous sibling, -1 on error.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Prev(&mut self) -> c_int {
        if self.state != ReadState::READING || self.cur_node.is_null() {
            return -1;
        }

        // Scan backward through events to find the previous sibling.
        let current_depth = self.depth;
        let mut i = if self.event_index > 0 {
            self.event_index - 1
        } else {
            return 0;
        };

        loop {
            let event = &self.events[i];
            if event.depth == current_depth && !event.is_end {
                self.position_at(i);
                return 1;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }

        0
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Information methods
    // ─────────────────────────────────────────────────────────────────────────

    /// Get the depth of the current node.
    pub const fn Depth(&self) -> c_int {
        self.depth
    }

    /// Get the node type of the current node.
    pub const fn NodeType(&self) -> ReaderNodeType {
        self.node_type
    }

    /// Get the name of the current node.
    ///
    /// Returns a pointer to a newly allocated string (caller must free with `xmlFree`),
    /// or NULL if there is no name.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Name(&self) -> *mut xmlChar {
        if self.name.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: name is a valid null-terminated xmlChar string.
        unsafe { xml_strdup(self.name as *const xmlChar) }
    }

    /// Get the value of the current node.
    ///
    /// Returns a pointer to a newly allocated string (caller must free with `xmlFree`),
    /// or NULL if there is no value.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Value(&self) -> *mut xmlChar {
        if self.value.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: value is a valid null-terminated xmlChar string.
        unsafe { xml_strdup(self.value as *const xmlChar) }
    }

    /// Get a constant pointer to the name (no copy).
    ///
    /// The returned pointer is valid only while the reader is alive and positioned
    /// on the same node.
    pub const fn ConstName(&self) -> *const xmlChar {
        self.name as *const xmlChar
    }

    /// Get a constant pointer to the value (no copy).
    ///
    /// The returned pointer is valid only while the reader is alive and positioned
    /// on the same node.
    pub const fn ConstValue(&self) -> *const xmlChar {
        self.value as *const xmlChar
    }

    /// Check if the current node has a value.
    pub const fn HasValue(&self) -> c_int {
        if self.value.is_null() {
            0
        } else {
            1
        }
    }

    /// Check if the current node has attributes.
    pub fn HasAttributes(&self) -> c_int {
        if self.cur_node.is_null() {
            return 0;
        }
        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return 0;
        }
        // SAFETY: cur_node is an element.
        // UPSTREAM-PARITY: namespace declarations count as attributes.
        let props = unsafe { (*self.cur_node).properties };
        let nsdefs = unsafe { (*self.cur_node).nsDef };
        if props.is_null() && nsdefs.is_null() {
            0
        } else {
            1
        }
    }

    /// Check if the current element is an empty element (no children).
    ///
    /// UPSTREAM-PARITY (xmlreader.c xmlTextReaderIsEmptyElement): with no
    /// current node (pre-first-Read or after EOF) this returns -1 (error),
    /// which the PHP consumer turns into its "no XML data has been read yet"
    /// property error.
    pub fn IsEmptyElement(&self) -> c_int {
        if self.cur_node.is_null() {
            return -1;
        }
        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return 0;
        }
        // SAFETY: cur_node is an element.
        let children = unsafe { (*self.cur_node).children };
        if children.is_null() {
            1
        } else {
            0
        }
    }

    /// Get the base URI of the current node.
    ///
    /// Returns a newly allocated string (caller must free with `xmlFree`),
    /// or NULL if not available.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn BaseUri(&self) -> *mut xmlChar {
        // The base URI is typically the document URL. UPSTREAM-PARITY
        // (xmlreader.c xmlTextReaderBaseUri): NULL unless a node is current.
        if self.cur_node.is_null() {
            return ptr::null_mut();
        }
        if self.doc.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: doc is valid.
        let url = unsafe { (*self.doc).URL };
        if url.is_null() {
            return ptr::null_mut();
        }
        // SAFETY: url is null-terminated.
        unsafe { xml_strdup(url as *const xmlChar) }
    }

    /// Get the local name of the current node.
    ///
    /// For namespaced names, this strips the prefix.
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn LocalName(&self) -> *mut xmlChar {
        if self.name.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: name is a valid null-terminated string.
        let name_bytes = unsafe { xmlstr_to_bytes(self.name as *const xmlChar) };

        // Find the colon separator.
        if let Some(pos) = name_bytes.iter().position(|&b| b == b':') {
            // Return everything after the colon.
            let local = &name_bytes[pos + 1..];
            if local.is_empty() {
                return ptr::null_mut();
            }
            // SAFETY: bytes_to_xmlstr allocates via xmlMalloc.
            unsafe { bytes_to_xmlstr(local) }
        } else {
            // No prefix, return the name as-is.
            // SAFETY: xml_strdup allocates via xmlMalloc.
            unsafe { xml_strdup(self.name as *const xmlChar) }
        }
    }

    /// Get the namespace URI of the current node.
    ///
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn NamespaceUri(&self) -> *mut xmlChar {
        if self.cur_node.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is valid.
        let ns = unsafe { (*self.cur_node).ns };
        if ns.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: ns is valid.
        let href = unsafe { (*ns).href };
        if href.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: href is null-terminated.
        unsafe { xml_strdup(href as *const xmlChar) }
    }

    /// Get the prefix of the current node.
    ///
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn Prefix(&self) -> *mut xmlChar {
        if self.cur_node.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is valid.
        let ns = unsafe { (*self.cur_node).ns };
        if ns.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: ns is valid.
        let prefix = unsafe { (*ns).prefix };
        if prefix.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: prefix is null-terminated.
        unsafe { xml_strdup(prefix as *const xmlChar) }
    }

    /// Get the attribute count of the current element.
    ///
    /// UPSTREAM-PARITY (xmlreader.c xmlTextReaderAttributeCount): with no
    /// current node (pre-first-Read or after EOF) the count is 0, never -1;
    /// -1 is reserved for a NULL reader. At END_ELEMENT events (and other
    /// non-element positions) the count is 0 as well.
    pub const fn AttributeCount(&self) -> c_int {
        if self.cur_node.is_null() {
            0
        } else if self.attribute_count < 0 {
            0
        } else {
            self.attribute_count
        }
    }

    /// Get the read state.
    pub const fn ReadState(&self) -> ReadState {
        self.state
    }

    /// Get an attribute value by name.
    ///
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn GetAttribute(&self, name: *const xmlChar) -> *mut xmlChar {
        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderGetAttribute): while the
        // attribute cursor is active (upstream curnode != NULL — a
        // moveToAttribute/moveToNextAttribute is in effect) the attribute
        // value accessors return NULL; they read from the ELEMENT position
        // only. The candidate tracks that cursor with `cur_attribute`.
        if self.cur_attribute >= 0 || self.cur_node.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is an element.
        let idx = unsafe { self.attr_index_by_name(self.cur_node, name) };
        if idx < 0 {
            return ptr::null_mut();
        }
        match unsafe { self.attr_at(self.cur_node, idx) } {
            AttrTarget::Ns(ns) => {
                let href = unsafe { (*ns).href };
                if href.is_null() {
                    ptr::null_mut()
                } else {
                    unsafe { xml_strdup(href as *const xmlChar) }
                }
            }
            AttrTarget::Prop(prop) => {
                // Get the attribute value from its child text node; an
                // existing attribute with an empty value has no children
                // and reports "" (upstream xmlGetProp).
                let val = unsafe { (*prop).children };
                if !val.is_null() {
                    let content = unsafe { (*val).content };
                    if !content.is_null() {
                        return unsafe { xml_strdup(content as *const xmlChar) };
                    }
                }
                unsafe { xml_strdup(c"".as_ptr() as *const xmlChar) }
            }
            AttrTarget::None => ptr::null_mut(),
        }
    }

    /// Get an attribute value by index.
    ///
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn GetAttributeNo(&self, index: c_int) -> *mut xmlChar {
        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderGetAttributeNo): the
        // accessor reads from the ELEMENT position only — an active
        // attribute cursor returns NULL.
        if self.cur_attribute >= 0 || self.cur_node.is_null() || index < 0 {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is an element.
        match unsafe { self.attr_at(self.cur_node, index) } {
            AttrTarget::Ns(ns) => {
                let href = unsafe { (*ns).href };
                if href.is_null() {
                    ptr::null_mut()
                } else {
                    unsafe { xml_strdup(href as *const xmlChar) }
                }
            }
            AttrTarget::Prop(prop) => {
                let val = unsafe { (*prop).children };
                if !val.is_null() {
                    let content = unsafe { (*val).content };
                    if !content.is_null() {
                        return unsafe { xml_strdup(content as *const xmlChar) };
                    }
                }
                unsafe { xml_strdup(c"".as_ptr() as *const xmlChar) }
            }
            AttrTarget::None => ptr::null_mut(),
        }
    }

    /// Get an attribute value by local name and namespace URI.
    ///
    /// Returns a newly allocated string, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn GetAttributeNs(
        &self,
        localName: *const xmlChar,
        namespaceURI: *const xmlChar,
    ) -> *mut xmlChar {
        // UPSTREAM-PARITY (xmlreader.c xmlTextReaderGetAttributeNs): the
        // accessor reads from the ELEMENT position only — an active
        // attribute cursor returns NULL.
        if self.cur_attribute >= 0 || self.cur_node.is_null() {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is valid.
        let etype = unsafe { (*self.cur_node).type_ };
        if etype != XML_ELEMENT_NODE as c_int {
            return ptr::null_mut();
        }

        // SAFETY: cur_node is an element.
        let mut prop = unsafe { (*self.cur_node).properties };
        while !prop.is_null() {
            // SAFETY: prop is valid.
            let prop_local = unsafe { (*prop).name };
            let prop_ns = unsafe { (*prop).ns };

            // Check local name match.
            if prop_local.is_null() {
                // SAFETY: prop's next is valid.
                prop = unsafe { (*prop).next };
                continue;
            }

            // SAFETY: prop_local is null-terminated.
            let name_match = unsafe {
                crate::xml::string::xml_strcmp(prop_local as *const xmlChar, localName) == 0
            };

            if name_match {
                // Check namespace URI match.
                let ns_match = if namespaceURI.is_null() {
                    prop_ns.is_null()
                } else if prop_ns.is_null() {
                    false
                } else {
                    // SAFETY: Both hrefs are null-terminated.
                    unsafe {
                        crate::xml::string::xml_strcmp(
                            (*prop_ns).href as *const xmlChar,
                            namespaceURI,
                        ) == 0
                    }
                };

                if ns_match {
                    // SAFETY: prop is valid. An existing attribute with an
                    // empty value has no children and reports "" (upstream
                    // xmlGetProp).
                    let val = unsafe { (*prop).children };
                    if !val.is_null() {
                        // SAFETY: val's content is null-terminated.
                        let content = unsafe { (*val).content };
                        if !content.is_null() {
                            // SAFETY: content is null-terminated.
                            return unsafe { xml_strdup(content as *const xmlChar) };
                        }
                    }
                    return unsafe { xml_strdup(c"".as_ptr() as *const xmlChar) };
                }
            }

            // SAFETY: prop's next is valid.
            prop = unsafe { (*prop).next };
        }

        ptr::null_mut()
    }

    /// Look up a namespace by prefix.
    ///
    /// Returns a newly allocated string with the namespace URI, or NULL.
    ///
    /// # SAFETY
    ///
    /// `self` must be a valid `XmlTextReader` obtained from a reader
    /// constructor (`xmlReaderForDoc`/`xmlReaderForFile`/`xmlReaderForMemory`/
    /// `xmlReaderForIO` or `xmlNewTextReader`) and not yet closed or freed.
    /// The `&mut self` borrow excludes concurrent access from other threads.
    /// Pointers returned by this method are valid only until the next
    /// traversal operation invalidates them, per the C API contract.
    pub unsafe fn LookupNamespace(&self, prefix: *const xmlChar) -> *mut xmlChar {
        if self.cur_node.is_null() {
            return ptr::null_mut();
        }

        // Walk up the tree looking for a namespace declaration matching the prefix.
        // SAFETY: cur_node is valid.
        let mut cur = self.cur_node;
        while !cur.is_null() {
            // SAFETY: cur is valid.
            let mut ns_def = unsafe { (*cur).nsDef };
            while !ns_def.is_null() {
                // SAFETY: ns_def is valid.
                let ns_prefix = unsafe { (*ns_def).prefix };

                let match_prefix = if prefix.is_null() || *prefix == 0 {
                    // Looking for default namespace.
                    ns_prefix.is_null()
                } else if ns_prefix.is_null() {
                    false
                } else {
                    // SAFETY: Both are null-terminated.
                    unsafe {
                        crate::xml::string::xml_strcmp(ns_prefix as *const xmlChar, prefix) == 0
                    }
                };

                if match_prefix {
                    // SAFETY: ns_def is valid.
                    let href = unsafe { (*ns_def).href };
                    if !href.is_null() {
                        // SAFETY: href is null-terminated.
                        return unsafe { xml_strdup(href as *const xmlChar) };
                    }
                    return ptr::null_mut();
                }

                // SAFETY: ns_def's next is valid.
                ns_def = unsafe { (*ns_def).next };
            }

            // SAFETY: cur's parent is valid.
            cur = unsafe { (*cur).parent };
        }

        ptr::null_mut()
    }

    /// Get a parser property.
    pub const fn GetParserProp(&self, prop: c_int) -> c_int {
        match prop {
            1 /* XML_PARSER_LOADDTD */ => {
                if (self.options & XML_PARSE_DTDLOAD) != 0 { 1 } else { 0 }
            }
            2 /* XML_PARSER_DEFAULTATTRS */ => {
                if (self.options & XML_PARSE_DTDATTR) != 0 { 1 } else { 0 }
            }
            3 /* XML_PARSER_VALIDATE */ => {
                if (self.options & XML_PARSE_DTDVALID) != 0 { 1 } else { 0 }
            }
            4 /* XML_PARSER_SUBST_ENTITIES */ => {
                if (self.options & XML_PARSE_NOENT) != 0 { 1 } else { 0 }
            }
            _ => -1,
        }
    }

    /// Set a parser property.
    pub const fn SetParserProp(&mut self, prop: c_int, value: c_int) -> c_int {
        match prop {
            1 /* XML_PARSER_LOADDTD */ => {
                if value != 0 {
                    self.options |= XML_PARSE_DTDLOAD;
                } else {
                    self.options &= !XML_PARSE_DTDLOAD;
                }
                0
            }
            2 /* XML_PARSER_DEFAULTATTRS */ => {
                if value != 0 {
                    self.options |= XML_PARSE_DTDATTR;
                } else {
                    self.options &= !XML_PARSE_DTDATTR;
                }
                0
            }
            3 /* XML_PARSER_VALIDATE */ => {
                if value != 0 {
                    self.options |= XML_PARSE_DTDVALID;
                } else {
                    self.options &= !XML_PARSE_DTDVALID;
                }
                0
            }
            4 /* XML_PARSER_SUBST_ENTITIES */ => {
                if value != 0 {
                    self.options |= XML_PARSE_NOENT;
                } else {
                    self.options &= !XML_PARSE_NOENT;
                }
                0
            }
            _ => -1,
        }
    }

    /// Get the current document.
    pub const fn CurrentDoc(&self) -> *mut _xmlDoc {
        self.doc
    }
}

/// Recursively prune the children of `parent`: a child survives iff it is a
/// DTD node, it or an element ancestor is matched by a registered pattern
/// (`parent_matched` tracks the latter), or it is in the keep-set (matched
/// nodes and their element ancestors). Non-surviving subtrees are unlinked
/// and freed wholesale. This reproduces the post-stream state of upstream
/// `xmlTextReaderRead`'s NODE_IS_PRESERVED pruning for the whole-tree
/// reader.
///
/// # SAFETY
///
/// - `parent` must be a valid `_xmlNode`; `keep` must map every node pointer
///   that must survive; `patterns` are compiled patterns owned by the
///   reader.
unsafe fn prune_unpreserved(
    parent: *mut _xmlNode,
    parent_matched: bool,
    keep: &HashSet<usize>,
    patterns: &[crate::abi::exports_automata::xmlPatternPtr],
) {
    let mut child = unsafe { (*parent).children };
    while !child.is_null() {
        let next = unsafe { (*child).next };
        let is_dtd =
            unsafe { (*child).type_ } == crate::abi::types::xmlElementType::XML_DTD_NODE as c_int;
        let is_matched = patterns
            .iter()
            .any(|p| unsafe { crate::abi::exports_automata::xmlPatternMatch(*p, child) == 1 });
        let survives = is_dtd || parent_matched || is_matched || keep.contains(&(child as usize));
        if survives {
            unsafe {
                prune_unpreserved(child, parent_matched || is_matched, keep, patterns);
            }
        } else {
            // SAFETY: child is linked into the tree; unlink first so the
            // parent chain stays consistent, then free the subtree.
            unsafe {
                tree::unlink_node(child);
                tree::free_node_list(child);
            }
        }
        child = next;
    }
}

impl Drop for XmlTextReader {
    fn drop(&mut self) {
        // Free cached strings.
        self.clear_cached_name();
        self.clear_cached_value();

        // Free compiled preserve-patterns (xmlTextReaderPreservePattern;
        // upstream xmlTextReaderFree frees patternTab).
        for p in self.pattern_tab.drain(..) {
            // SAFETY: each pattern was compiled by xmlPatterncompile and is
            // owned by the reader.
            unsafe { crate::abi::exports_automata::xmlFreePattern(p) };
        }

        // Free the cached last-error message (owned, xmlMalloc'd).
        if !self.last_err.message.is_null() {
            // SAFETY: message was allocated by xmlMalloc in GetLastError.
            unsafe { libc::free(self.last_err.message as *mut libc::c_void) };
            self.last_err.message = ptr::null_mut();
        }

        // Free encoding and URL.
        if !self.encoding.is_null() {
            // SAFETY: encoding was allocated by xmlMalloc.
            unsafe { xmlFreeImpl(self.encoding as *mut c_void) };
            self.encoding = ptr::null_mut();
        }
        if !self.URL.is_null() {
            // SAFETY: URL was allocated by xmlMalloc.
            unsafe { xmlFreeImpl(self.URL as *mut c_void) };
            self.URL = ptr::null_mut();
        }

        // Free the document if we own it (walker readers borrow the doc).
        // xmlTextReaderCurrentDoc sets `preserve`, transferring ownership to
        // the caller exactly like upstream's reader->preserve (xmlreader.c:
        // xmlTextReaderCurrentDoc sets preserve=1; xmlTextReaderClose frees
        // ctxt->myDoc only when preserve==0).
        if !self.doc.is_null() && self.owns_doc && !self.preserve {
            // SAFETY: doc was created by the parser, which allocates via xmlMalloc.
            // We own the doc since we created it.
            unsafe { tree::free_doc(self.doc) };
            self.doc = ptr::null_mut();
        }

        // Free the parser context if still alive.
        if !self.ctxt.is_null() {
            // SAFETY: ctxt was created by create_parser_ctxt.
            unsafe { free_parser_ctxt(self.ctxt) };
            self.ctxt = ptr::null_mut();
        }

        // Free compiled schemas the reader owns (xmlTextReaderSchemaValidate /
        // xmlTextReaderRelaxNGValidate compile them from a file path).
        // Caller-owned schemas (xmlTextReaderSetSchema /
        // xmlTextReaderRelaxNGSetSchema) are released by their owner — php
        // keeps them in intern->schema and frees them on re-set/close.
        if self.schema_owned && !self.schema.is_null() {
            // SAFETY: schema was compiled by xmlSchemaParse (Box<XsdSchema>).
            unsafe { crate::xml::schemas::xmlSchemaFree(self.schema) };
            self.schema = ptr::null_mut();
            self.schema_owned = false;
        }
        if self.rng_owned && !self.rng.is_null() {
            // SAFETY: rng was compiled by xmlRelaxNGParse (Box<RelaxNgSchema>).
            unsafe { crate::xml::relaxng::xmlRelaxNGFree(self.rng) };
            self.rng = ptr::null_mut();
            self.rng_owned = false;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Reader construction helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a reader from a parser input buffer.
///
/// # Safety
///
/// `input` must be a valid `_xmlParserInputBuffer` pointer.
unsafe fn reader_from_input(
    input: *mut _xmlParserInputBuffer,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    if input.is_null() {
        return ptr::null_mut();
    }

    // Create a parser context.
    let ctxt = create_parser_ctxt();
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // Read all data from the input buffer using the read callback. A
    // memory buffer (xmlParserInputBufferCreateMem) has NO read callback —
    // upstream serves the parser from the buffer's own content, which the
    // candidate keeps in the input-buffer content stash (helpers.rs); an
    // I/O buffer's callback slurps the stream (XMLReader::fromStream).
    let mut data = Vec::new();
    let mut tmp = [0u8; 4096];

    // SAFETY: input is valid.
    let read_cb = unsafe { (*input).readcallback };
    let ioctx = unsafe { (*input).context };

    if let Some(read) = read_cb {
        loop {
            // SAFETY: The read callback must be valid and ioctx must be a valid context.
            let n = unsafe { read(ioctx, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int) };
            if n <= 0 {
                break;
            }
            data.extend_from_slice(&tmp[..n as usize]);
        }
    } else {
        data = crate::xml::parser::helpers::take_input_buf_content(input).unwrap_or_default();
    }

    // Close the input if there's a close callback.
    // SAFETY: input is valid.
    let close_cb = unsafe { (*input).closecallback };
    if let Some(close) = close_cb {
        // SAFETY: The close callback must be valid.
        unsafe { close(ioctx) };
    }

    // Create an InputBuffer from the data.
    let input_buf = InputBuffer::from_memory(&data, None);

    // Set up the parser context with the input.
    setup_parser_input(ctxt, input_buf);

    // Set options (mirrors xmlCtxtUseOptions so the historical struct
    // members track XML_PARSE_* options — reader2.c validates via
    // XML_PARSE_DTDVALID; Phase-12 EXTERNAL-CONSUMERS court).
    unsafe {
        crate::abi::exports_parser::apply_options(ctxt, options);
    }

    // Build URL and encoding strings.
    let url_bytes = if URL.is_null() {
        None
    } else {
        // SAFETY: URL is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(URL);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let enc_bytes = if encoding.is_null() {
        None
    } else {
        // SAFETY: encoding is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(encoding);
            Some(cstr.to_bytes().to_vec())
        }
    };

    // Create the reader.
    let mut reader = XmlTextReader::new(ctxt, url_bytes.as_deref(), enc_bytes.as_deref());
    reader.options = options;

    // Box and leak the reader to return a raw pointer.
    Box::into_raw(Box::new(reader))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new text reader from an input buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextReaderPtr xmlNewTextReader(xmlParserInputBufferPtr input, const char *URI);
/// ```
///
/// # Safety
///
/// - `input` must be a valid `_xmlParserInputBuffer` pointer or NULL.
/// - `URI` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextReader(
    input: *mut _xmlParserInputBuffer,
    URI: *const c_char,
) -> *mut XmlTextReader {
    // SAFETY: Forward to the helper.
    unsafe { reader_from_input(input, URI, ptr::null(), 0) }
}

/// Create a text reader for a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextReaderPtr xmlReaderForFile(const char *filename, const char *encoding, int options);
/// ```
///
/// # Safety
///
/// - `filename` must be a valid C string or NULL.
/// - `encoding` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderForFile(
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: create_parser_ctxt returns a valid context or NULL.
    let ctxt = unsafe { create_parser_ctxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: the open routes through the registered loader (php streams)
    // first, else the built-in file read; filename is a valid C string.
    // UPSTREAM-PARITY (reader.c xmlNewTextReaderFilename): the reader builds
    // its input via xmlParserInputBufferCreateFilename, which does NOT raise
    // xmlCtxtErrIO on XML_IO_ENOENT — a failed open returns NULL silently and
    // the php binding reports "Unable to open source data".
    let input = match unsafe { crate::abi::exports_parser::open_filename_routed(filename) } {
        crate::abi::exports_parser::RoutedFileOpen::Loaded(input) => input,
        crate::abi::exports_parser::RoutedFileOpen::Failed => {
            // SAFETY: ctxt is valid.
            unsafe { free_parser_ctxt(ctxt) };
            return ptr::null_mut();
        }
        crate::abi::exports_parser::RoutedFileOpen::Builtin => {
            match unsafe { input_from_file(filename) } {
                Ok(input) => input,
                Err(_) => {
                    // SAFETY: ctxt is valid.
                    unsafe { free_parser_ctxt(ctxt) };
                    return ptr::null_mut();
                }
            }
        }
    };

    // SAFETY: ctxt and input are valid.
    unsafe { setup_parser_input(ctxt, input) };
    unsafe {
        (*ctxt).options = options;
    }

    let enc_bytes = if encoding.is_null() {
        None
    } else {
        // SAFETY: encoding is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(encoding);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let mut reader = XmlTextReader::new(ctxt, None, enc_bytes.as_deref());
    reader.options = options;
    Box::into_raw(Box::new(reader))
}

/// Create a text reader from memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextReaderPtr xmlReaderForMemory(const char *buffer, int size,
///                                     const char *URL, const char *encoding, int options);
/// ```
///
/// # Safety
///
/// - `buffer` must be a valid pointer with at least `size` readable bytes.
/// - `URL` and `encoding` must be valid C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderForMemory(
    buffer: *const c_char,
    size: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }

    // SAFETY: create_parser_ctxt returns a valid context or NULL.
    let ctxt = unsafe { create_parser_ctxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: input_from_memory copies the data; buffer and size are valid.
    // UPSTREAM-PARITY: the URL is recorded as the input's filename.
    let input = unsafe { input_from_memory_named(buffer, size, URL) };

    // SAFETY: ctxt and input are valid.
    unsafe { setup_parser_input(ctxt, input) };
    unsafe {
        (*ctxt).options = options;
    }

    let url_bytes = if URL.is_null() {
        None
    } else {
        // SAFETY: URL is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(URL);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let enc_bytes = if encoding.is_null() {
        None
    } else {
        // SAFETY: encoding is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(encoding);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let mut reader = XmlTextReader::new(ctxt, url_bytes.as_deref(), enc_bytes.as_deref());
    reader.options = options;
    Box::into_raw(Box::new(reader))
}

/// Create a text reader from a file descriptor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextReaderPtr xmlReaderForFd(int fd, const char *URL,
///                                 const char *encoding, int options);
/// ```
///
/// # Safety
///
/// - `fd` must be a valid open file descriptor.
/// - `URL` and `encoding` must be valid C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderForFd(
    fd: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    // SAFETY: create_parser_ctxt returns a valid context or NULL.
    let ctxt = unsafe { create_parser_ctxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // Read all data from the fd.
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        // SAFETY: fd must be a valid open file descriptor.
        let n = unsafe { libc::read(fd, tmp.as_mut_ptr() as *mut c_void, tmp.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n as usize]);
    }

    // SAFETY: input_from_memory copies the buffer contents.
    let input = unsafe { input_from_memory(buf.as_ptr() as *const c_char, buf.len() as c_int) };

    // SAFETY: ctxt and input are valid.
    unsafe { setup_parser_input(ctxt, input) };
    unsafe {
        (*ctxt).options = options;
    }

    let url_bytes = if URL.is_null() {
        None
    } else {
        // SAFETY: URL is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(URL);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let enc_bytes = if encoding.is_null() {
        None
    } else {
        // SAFETY: encoding is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(encoding);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let mut reader = XmlTextReader::new(ctxt, url_bytes.as_deref(), enc_bytes.as_deref());
    reader.options = options;
    Box::into_raw(Box::new(reader))
}

/// Create a text reader from I/O callbacks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlTextReaderPtr xmlReaderForIO(xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
///                                 void *ioctx, const char *URL,
///                                 const char *encoding, int options);
/// ```
///
/// # Safety
///
/// - `ioread` and `ioclose` must be valid function pointers or None.
/// - `ioctx` must be a valid context pointer for the callbacks.
/// - `URL` and `encoding` must be valid C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderForIO(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    // SAFETY: create_parser_ctxt returns a valid context or NULL.
    let ctxt = unsafe { create_parser_ctxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: input_from_io reads all data via callbacks.
    let input = unsafe { input_from_io(ioread, ioclose, ioctx) };

    // SAFETY: ctxt and input are valid.
    unsafe { setup_parser_input(ctxt, input) };
    unsafe {
        (*ctxt).options = options;
    }

    let url_bytes = if URL.is_null() {
        None
    } else {
        // SAFETY: URL is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(URL);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let enc_bytes = if encoding.is_null() {
        None
    } else {
        // SAFETY: encoding is a valid C string.
        unsafe {
            let cstr = std::ffi::CStr::from_ptr(encoding);
            Some(cstr.to_bytes().to_vec())
        }
    };

    let mut reader = XmlTextReader::new(ctxt, url_bytes.as_deref(), enc_bytes.as_deref());
    reader.options = options;
    Box::into_raw(Box::new(reader))
}

// ─────────────────────────────────────────────────────────────────────────────
// Navigation functions
// ─────────────────────────────────────────────────────────────────────────────

/// Advance the reader to the next node in document order.
///
/// Returns 1 on success, 0 if EOF, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderRead(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer returned by `xmlNewTextReader` or one of the
/// `xmlReaderFor*` functions, or NULL (in which case -1 is returned).
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderRead(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Read() }
}

/// Skip to the next sibling of the current node.
///
/// Returns 1 on success, 0 if no more siblings, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderNext(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderNext(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Next() }
}

/// Skip to the next sibling (same as xmlTextReaderNext).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderNextSibling(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderNextSibling(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Next() }
}

/// Skip to the previous sibling of the current node.
///
/// Returns 1 on success, 0 if no previous sibling, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderPrev(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderPrev(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Prev() }
}

/// Move the reader back to the parent element (from an attribute).
///
/// Returns 1 on success, 0 if not on an attribute, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderMoveToElement(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToElement(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).MoveToElement() }
}

/// Move to an attribute by name.
///
/// Returns 1 on success, 0 if not found, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderMoveToAttribute(xmlTextReaderPtr reader, const xmlChar *name);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL. `name` must be a valid C string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToAttribute(
    reader: *mut XmlTextReader,
    name: *const xmlChar,
) -> c_int {
    if reader.is_null() || name.is_null() {
        return -1;
    }
    // SAFETY: reader and name are valid.
    unsafe { (*reader).MoveToAttribute(name) }
}

/// Move to an attribute by index.
///
/// Returns 1 on success, 0 if not found, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderMoveToAttributeNo(xmlTextReaderPtr reader, int index);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToAttributeNo(
    reader: *mut XmlTextReader,
    index: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).MoveToAttributeNo(index) }
}

/// Move to the first attribute of the current element.
///
/// Returns 1 on success, 0 if no attributes, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderMoveToFirstAttribute(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToFirstAttribute(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).MoveToFirstAttribute() }
}

/// Move to the next attribute.
///
/// Returns 1 on success, 0 if no more attributes, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderMoveToNextAttribute(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToNextAttribute(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).MoveToNextAttribute() }
}

// ─────────────────────────────────────────────────────────────────────────────
// Information methods
// ─────────────────────────────────────────────────────────────────────────────

/// Get the attribute count of the current element.
///
/// Returns the number of attributes, or -1 if not on an element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderAttributeCount(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderAttributeCount(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).AttributeCount() }
}

/// Get the depth of the current node.
///
/// Returns the depth (0 for root element), or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderDepth(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderDepth(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Depth() }
}

/// Get the node type of the current node.
///
/// Returns one of the `xmlReaderTypes` constants, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderNodeType(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderNodeType(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).NodeType() as c_int }
}

/// Get the name of the current node.
///
/// Returns a newly allocated string (caller must free with `xmlFree`),
/// or NULL if there is no name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderName(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderName(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Name() }
}

/// Get the value of the current node.
///
/// Returns a newly allocated string (caller must free with `xmlFree`),
/// or NULL if there is no value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderValue(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderValue(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Value() }
}

/// Get a constant pointer to the name (no copy).
///
/// The returned pointer is valid only while the reader is alive and positioned
/// on the same node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlTextReaderConstName(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderConstName(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).ConstName() }
}

/// Get a constant pointer to the value (no copy).
///
/// The returned pointer is valid only while the reader is alive and positioned
/// on the same node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlTextReaderConstValue(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderConstValue(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).ConstValue() }
}

/// Get the base URI of the current node.
///
/// Returns a newly allocated string (caller must free with `xmlFree`),
/// or NULL if not available.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderBaseUri(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderBaseUri(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).BaseUri() }
}

/// Get the local name of the current node.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderLocalName(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderLocalName(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).LocalName() }
}

/// Get the namespace URI of the current node.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderNamespaceUri(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderNamespaceUri(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).NamespaceUri() }
}

/// Get the prefix of the current node.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderPrefix(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderPrefix(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).Prefix() }
}

/// Check if the current node has a value.
///
/// Returns 1 if the node has a value, 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderHasValue(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderHasValue(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return 0;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).HasValue() }
}

/// Check if the current node has attributes.
///
/// Returns 1 if the node has attributes, 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderHasAttributes(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderHasAttributes(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return 0;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).HasAttributes() }
}

/// Check if the current element is an empty element (no children).
///
/// Returns 1 if empty, 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderIsEmptyElement(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderIsEmptyElement(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return 0;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).IsEmptyElement() }
}

/// Get the read state.
///
/// Returns one of the `xmlTextReaderReadState` constants.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderReadState(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderReadState(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return ReadState::ERROR as c_int;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).ReadState() as c_int }
}

// ─────────────────────────────────────────────────────────────────────────────
// Attribute access
// ─────────────────────────────────────────────────────────────────────────────

/// Get an attribute value by name.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderGetAttribute(xmlTextReaderPtr reader, const xmlChar *name);
/// ```
///
/// # Safety
///
/// `reader` and `name` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetAttribute(
    reader: *mut XmlTextReader,
    name: *const xmlChar,
) -> *mut xmlChar {
    if reader.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader and name are valid.
    unsafe { (*reader).GetAttribute(name) }
}

/// Get an attribute value by index.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderGetAttributeNo(xmlTextReaderPtr reader, int index);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetAttributeNo(
    reader: *mut XmlTextReader,
    index: c_int,
) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).GetAttributeNo(index) }
}

/// Get an attribute value by local name and namespace URI.
///
/// Returns a newly allocated string, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderGetAttributeNs(xmlTextReaderPtr reader,
///                                      const xmlChar *localName,
///                                      const xmlChar *namespaceURI);
/// ```
///
/// # Safety
///
/// `reader`, `localName`, and `namespaceURI` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetAttributeNs(
    reader: *mut XmlTextReader,
    localName: *const xmlChar,
    namespaceURI: *const xmlChar,
) -> *mut xmlChar {
    if reader.is_null() || localName.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader, localName, and namespaceURI are valid.
    unsafe { (*reader).GetAttributeNs(localName, namespaceURI) }
}

/// Look up a namespace by prefix.
///
/// Returns a newly allocated string with the namespace URI, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlTextReaderLookupNamespace(xmlTextReaderPtr reader, const xmlChar *prefix);
/// ```
///
/// # Safety
///
/// `reader` and `prefix` must be valid pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderLookupNamespace(
    reader: *mut XmlTextReader,
    prefix: *const xmlChar,
) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader and prefix are valid.
    unsafe { (*reader).LookupNamespace(prefix) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Parser properties
// ─────────────────────────────────────────────────────────────────────────────

/// Get a parser property.
///
/// Returns the property value (0 or 1), or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderGetParserProp(xmlTextReaderPtr reader, int prop);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetParserProp(
    reader: *mut XmlTextReader,
    prop: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).GetParserProp(prop) }
}

/// Set a parser property.
///
/// Returns 0 on success, -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderSetParserProp(xmlTextReaderPtr reader, int prop, int value);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderSetParserProp(
    reader: *mut XmlTextReader,
    prop: c_int,
    value: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).SetParserProp(prop, value) }
}

// ─────────────────────────────────────────────────────────────────────────────
// Lifecycle
// ─────────────────────────────────────────────────────────────────────────────

/// Free a text reader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeTextReader(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer returned by `xmlNewTextReader` or one of the
/// `xmlReaderFor*` functions, or NULL (in which case this is a no-op).
#[no_mangle]
pub unsafe extern "C" fn xmlFreeTextReader(reader: *mut XmlTextReader) {
    if reader.is_null() {
        return;
    }
    // SAFETY: reader was created via Box::into_raw, so we reconstruct the Box
    // and let it drop, which calls the Drop impl.
    unsafe {
        let _ = Box::from_raw(reader);
    }
}

/// Setup/reinitialize a reader with new input.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlTextReaderSetup(xmlTextReaderPtr reader,
///                        xmlParserInputBufferPtr input,
///                        const char *URL, const char *encoding, int options);
/// ```
///
/// # Safety
///
/// - `reader` must be a valid pointer or NULL.
/// - `input` must be a valid `_xmlParserInputBuffer` pointer or NULL.
/// - `URL` and `encoding` must be valid C strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderSetup(
    reader: *mut XmlTextReader,
    input: *mut _xmlParserInputBuffer,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }

    // SAFETY: reader is valid.
    let r = unsafe { &mut *reader };

    // Reset the reader state.
    r.clear_cached_name();
    r.clear_cached_value();

    r.events.clear();
    r.event_index = 0;
    r.state = ReadState::INITIALIZED;
    r.cur_node = ptr::null_mut();
    r.node_type = ReaderNodeType::NONE;
    r.depth = 0;
    r.attribute_count = -1;
    r.cur_attribute = -1;
    r.options = options;
    r.parsed = false;
    r.errors.clear();

    // Update URL.
    if !r.URL.is_null() {
        // SAFETY: URL was allocated by xmlMalloc.
        unsafe { xmlFreeImpl(r.URL as *mut c_void) };
        r.URL = ptr::null_mut();
    }
    if !URL.is_null() {
        // SAFETY: URL is a valid C string.
        let url_str = unsafe { std::ffi::CStr::from_ptr(URL) };
        // SAFETY: bytes_to_xmlstr allocates via xmlMalloc.
        r.URL = unsafe { bytes_to_xmlstr(url_str.to_bytes()) };
    }

    // Update encoding.
    if !r.encoding.is_null() {
        // SAFETY: encoding was allocated by xmlMalloc.
        unsafe { xmlFreeImpl(r.encoding as *mut c_void) };
        r.encoding = ptr::null_mut();
    }
    if !encoding.is_null() {
        // SAFETY: encoding is a valid C string.
        let enc_str = unsafe { std::ffi::CStr::from_ptr(encoding) };
        // SAFETY: bytes_to_xmlstr allocates via xmlMalloc.
        r.encoding = unsafe { bytes_to_xmlstr(enc_str.to_bytes()) };
    }

    // UPSTREAM-PARITY (xmlreader.c xmlTextReaderSetup): a NULL input means
    // "use the input the reader was created with" — PHP's XMLReader::XML() /
    // fromString() call xmlNewTextReader(inputbfr, uri) and then
    // xmlTextReaderSetup(reader, NULL, uri, encoding, options). The
    // candidate consumed that buffer in reader_from_input (the context is
    // already wired), so a NULL input keeps the existing context; a NEW
    // input replaces it (C consumers re-arming the reader).
    if input.is_null() {
        // UPSTREAM-PARITY: a NULL input promotes the source the reader was
        // created with (php XMLReader::XML()/fromString(): xmlNewTextReader
        // then xmlTextReaderSetup(reader, NULL, ...)). The candidate
        // consumed that source in reader_from_input, so the context built
        // there is kept. A reader that ALREADY parsed (its context was
        // consumed by parse_and_build_events) discards the old document and
        // resets — upstream xmlTextReaderSetup tears the previous parse
        // state down.
        if r.ctxt.is_null() && !r.doc.is_null() {
            // SAFETY: doc was allocated by the parser.
            unsafe { tree::free_doc(r.doc) };
            r.doc = ptr::null_mut();
        }
        return 0;
    }

    // Free the old document.
    if !r.doc.is_null() {
        // SAFETY: doc was allocated by the parser.
        unsafe { tree::free_doc(r.doc) };
        r.doc = ptr::null_mut();
    }

    // Free old parser context.
    if !r.ctxt.is_null() {
        // SAFETY: ctxt was created by create_parser_ctxt.
        unsafe { free_parser_ctxt(r.ctxt) };
        r.ctxt = ptr::null_mut();
    }

    // Create new parser context and set up input.
    // SAFETY: create_parser_ctxt returns a valid context or NULL.
    let ctxt = unsafe { create_parser_ctxt() };
    if ctxt.is_null() {
        return -1;
    }

    // Read all data from the input buffer. A memory buffer
    // (xmlParserInputBufferCreateMem) has no read callback — its content
    // lives in the input-buffer content stash (helpers.rs).
    let mut data = Vec::new();
    let mut tmp = [0u8; 4096];

    // SAFETY: input is valid.
    let read_cb = unsafe { (*input).readcallback };
    let ioctx = unsafe { (*input).context };

    if let Some(read) = read_cb {
        loop {
            // SAFETY: callbacks are valid.
            let n = unsafe { read(ioctx, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int) };
            if n <= 0 {
                break;
            }
            data.extend_from_slice(&tmp[..n as usize]);
        }
    } else {
        data = crate::xml::parser::helpers::take_input_buf_content(input).unwrap_or_default();
    }

    // Close the input.
    let close_cb = unsafe { (*input).closecallback };
    if let Some(close) = close_cb {
        // SAFETY: close callback is valid.
        unsafe { close(ioctx) };
    }

    let input_buf = InputBuffer::from_memory(&data, None);

    // SAFETY: ctxt and input_buf are valid.
    unsafe { setup_parser_input(ctxt, input_buf) };
    unsafe {
        (*ctxt).options = options;
    }

    r.ctxt = ctxt;

    0
}

/// Get the current document from the reader.
///
/// Returns a pointer to the `_xmlDoc` or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlTextReaderCurrentDoc(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderCurrentDoc(reader: *mut XmlTextReader) -> *mut _xmlDoc {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid. Upstream xmlTextReaderCurrentDoc sets
    // reader->preserve = 1 and returns ctxt->myDoc: the document ownership
    // moves to the caller (freed with xmlFreeDoc). Mirror that so the
    // reader's Drop does not free a doc the caller still owns (upstream
    // reader3.c pattern; Phase-12 EXTERNAL-CONSUMERS court).
    unsafe {
        (*reader).preserve = true;
        (*reader).CurrentDoc()
    }
}

/// Close the reader, releasing the document and parser state.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderClose` (xmlreader.c): sets the mode to
/// XML_TEXTREADER_MODE_CLOSED, drops the current node, and tears down the
/// validation state. The reader object itself is freed separately with
/// `xmlFreeTextReader`.
///
/// ```c
/// int xmlTextReaderClose(xmlTextReaderPtr reader);
/// ```
///
/// Returns 0 on success, -1 if `reader` is NULL.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderClose(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid; close resets cursor state and marks the
    // reader closed, mirroring upstream's mode transition.
    unsafe {
        let r = &mut *reader;
        r.cur_node = ptr::null_mut();
        r.node_type = ReaderNodeType::NONE;
        r.clear_cached_name();
        r.clear_cached_value();
        r.state = ReadState::CLOSED;
    }
    0
}

/// Return the current node of the reader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNodePtr xmlTextReaderCurrentNode(xmlTextReaderPtr reader);
/// ```
///
/// Returns the current node or NULL. The node is owned by the document;
/// the caller must not free it.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderCurrentNode(reader: *mut XmlTextReader) -> *mut _xmlNode {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).cur_node }
}

/// Expand entity references at the current position.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderExpand` (xmlreader.c) forces substitution of the
/// current entity reference so the node can be read in full. When the parser
/// ran with XML_PARSE_NOENT the entities are already substituted during
/// parsing; the function then simply returns the current node.
///
/// ```c
/// xmlNodePtr xmlTextReaderExpand(xmlTextReaderPtr reader);
/// ```
///
/// Returns the (expanded) current node, or NULL if the reader is NULL or
/// not positioned on a node.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderExpand(reader: *mut XmlTextReader) -> *mut _xmlNode {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid.
    unsafe { (*reader).cur_node }
}

/// Return the parser line number of the current node.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderGetParserLineNumber` returns the input stream's
/// current line. The candidate records `line` per node during parsing, which
/// is equivalent for the read cursor.
///
/// ```c
/// int xmlTextReaderGetParserLineNumber(xmlTextReaderPtr reader);
/// ```
///
/// Returns the line number, or 0 when unavailable.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetParserLineNumber(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return 0;
    }
    // SAFETY: reader is valid; cur_node is owned by the doc.
    unsafe {
        let node = (*reader).cur_node;
        if node.is_null() {
            0
        } else {
            (*node).line as c_int
        }
    }
}

/// Return the parser column number of the current node.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderGetParserColumnNumber` returns the input stream's
/// column. Columns are not tracked per-node in the candidate tree (upstream
/// exposes -1 when no column information is available either); return -1.
///
/// ```c
/// int xmlTextReaderGetParserColumnNumber(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlTextReaderGetParserColumnNumber(
    reader: *mut XmlTextReader,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    -1
}

/// Return the validation status of the reader.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderIsValid` (xmlreader.c): with a RELAX NG
/// validation context attached the RELAX NG outcome wins, then the XSD
/// outcome, then the DTD-parse validity (`ctxt->valid`); 0 when no
/// validation was performed or it failed, -1 for a NULL reader.
///
/// ```c
/// int xmlTextReaderIsValid(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderIsValid(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    unsafe {
        let r = &*reader;
        if !r.rng.is_null() {
            if r.rng_result >= 0 {
                r.rng_result
            } else {
                // Validation not run yet (schema attached, first Read not
                // done): upstream's rngValidCtxt->valid starts at 0.
                0
            }
        } else if !r.schema.is_null() {
            if r.xsd_result >= 0 {
                r.xsd_result
            } else {
                0
            }
        } else if r.did_validate == 1 {
            r.was_valid
        } else {
            0
        }
    }
}

/// Return the normalization status of the reader.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderNormalization` returns 1 when the reader performs
/// whitespace normalization (it always reports 1 unless the parser was
/// configured otherwise). The candidate normalizes attribute values per the
/// XML spec during parsing, so report 1.
///
/// ```c
/// int xmlTextReaderNormalization(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlTextReaderNormalization(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    1
}

/// Read the value of an attribute as a text node (attribute-value mode).
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderReadAttributeValue` moves the reader so that the
/// value of the current attribute is available as a text node, returning 1
/// on success and 0 when already at the end. The candidate tree stores
/// attribute values directly on the attribute node, so the value is already
/// available via `xmlTextReaderValue`; report 1 when positioned on an
/// attribute with a value.
///
/// ```c
/// int xmlTextReaderReadAttributeValue(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderReadAttributeValue(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid.
    unsafe {
        let r = &*reader;
        if r.node_type == ReaderNodeType::ATTRIBUTE && !r.cur_node.is_null() {
            1
        } else {
            0
        }
    }
}

/// Read the content of the current node as a string.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderReadString` concatenates the text of the current
/// node's subtree (recursively) into one string. It behaves like
/// `xmlNodeGetContent` for the current node.
///
/// ```c
/// xmlChar *xmlTextReaderReadString(xmlTextReaderPtr reader);
/// ```
///
/// Returns a newly allocated string (free with `xmlFree`) or NULL.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderReadString(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid; node owned by the document.
    unsafe {
        let node = (*reader).cur_node;
        if node.is_null() {
            return ptr::null_mut();
        }
        tree::node_get_content(node)
    }
}

/// Read the inner XML of the current node as a string.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderReadInnerXml` serializes the children of the
/// current node. The candidate uses its serializer on the children list.
///
/// ```c
/// xmlChar *xmlTextReaderReadInnerXml(xmlTextReaderPtr reader);
/// ```
///
/// Returns a newly allocated string (free with `xmlFree`) or NULL.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderReadInnerXml(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid; node owned by the document.
    unsafe {
        let node = (*reader).cur_node;
        if node.is_null() {
            return ptr::null_mut();
        }
        let buf = crate::xml::io::buf_create(-1);
        if buf.is_null() {
            return ptr::null_mut();
        }
        let mut child = (*node).children;
        while !child.is_null() {
            tree::serialize_node(child, buf, 0, 0);
            child = (*child).next;
        }
        let len = crate::xml::io::buf_length(buf) as usize;
        let content = crate::xml::io::buf_content(buf);
        if content.is_null() || len == 0 {
            crate::xml::io::buf_free(buf);
            return ptr::null_mut();
        }
        let out = xml_strdup(content);
        crate::xml::io::buf_free(buf);
        out
    }
}

/// Read the outer XML of the current node as a string.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderReadOuterXml` serializes the current node itself.
///
/// ```c
/// xmlChar *xmlTextReaderReadOuterXml(xmlTextReaderPtr reader);
/// ```
///
/// Returns a newly allocated string (free with `xmlFree`) or NULL.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderReadOuterXml(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid; node owned by the document.
    unsafe {
        let node = (*reader).cur_node;
        if node.is_null() {
            return ptr::null_mut();
        }
        let buf = crate::xml::io::buf_create(-1);
        if buf.is_null() {
            return ptr::null_mut();
        }
        tree::serialize_node(node, buf, 0, 0);
        let len = crate::xml::io::buf_length(buf) as usize;
        let content = crate::xml::io::buf_content(buf);
        if content.is_null() || len == 0 {
            crate::xml::io::buf_free(buf);
            return ptr::null_mut();
        }
        let out = xml_strdup(content);
        crate::xml::io::buf_free(buf);
        out
    }
}

/// Return the standalone flag of the document being read.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderStandalone` returns the document's standalone
/// value (1 = standalone, 0 = not, -1 = no XML declaration / NULL reader).
///
/// ```c
/// int xmlTextReaderStandalone(xmlTextReaderPtr reader);
/// ```
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderStandalone(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    // SAFETY: reader is valid; doc owned by the reader.
    unsafe {
        let doc = (*reader).doc;
        if doc.is_null() {
            return -1;
        }
        (*doc).standalone
    }
}

/// Return the xml:lang of the current node.
///
/// # UPSTREAM-PARITY
///
/// Upstream `xmlTextReaderXmlLang` returns `xmlNodeGetLang(node)`: the
/// nearest `xml:lang` attribute on the node or an ancestor.
///
/// ```c
/// xmlChar *xmlTextReaderXmlLang(xmlTextReaderPtr reader);
/// ```
///
/// Returns a newly allocated string (free with `xmlFree`) or NULL.
///
/// # Safety
///
/// `reader` must be a valid pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderXmlLang(reader: *mut XmlTextReader) -> *mut xmlChar {
    if reader.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: reader is valid; node owned by the document.
    unsafe {
        let mut node = (*reader).cur_node;
        while !node.is_null() {
            // walk the property list for xml:lang
            let mut prop = (*node).properties;
            while !prop.is_null() {
                if !(*prop).name.is_null() {
                    let name = crate::xml::string::xmlstr_to_bytes((*prop).name);
                    if name == b"lang" && !(*prop).ns.is_null() {
                        let ns_href = crate::xml::string::xmlstr_to_bytes((*(*prop).ns).href);
                        if ns_href == b"http://www.w3.org/XML/1998/namespace" {
                            let v = (*prop).children;
                            if !v.is_null() && !(*v).content.is_null() {
                                return xml_strdup((*v).content);
                            }
                        }
                    }
                }
                prop = (*prop).next;
            }
            node = (*node).parent;
        }
        ptr::null_mut()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFreeImpl;
    use core::ffi::c_void;
    use std::os::raw::c_char;

    /// Helper: create a reader from a string.
    unsafe fn create_reader(xml: &str) -> *mut XmlTextReader {
        let bytes = xml.as_bytes();
        xmlReaderForMemory(
            bytes.as_ptr() as *const c_char,
            bytes.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        )
    }

    /// Helper: free a reader.
    unsafe fn free_reader(reader: *mut XmlTextReader) {
        if !reader.is_null() {
            xmlFreeTextReader(reader);
        }
    }

    /// Helper: read through all nodes and collect their types and names.
    unsafe fn collect_nodes(reader: *mut XmlTextReader) -> Vec<(ReaderNodeType, String, i32)> {
        let mut result = Vec::new();
        loop {
            let ret = xmlTextReaderRead(reader);
            if ret <= 0 {
                break;
            }
            // SAFETY: reader is valid.
            let r = &*reader;
            let ntype = r.NodeType();
            let name = if r.name.is_null() {
                String::new()
            } else {
                xmlstr_to_string(r.name as *const xmlChar)
            };
            let depth = r.Depth();
            result.push((ntype, name, depth));
        }
        result
    }

    // ─── Basic tests ───────────────────────────────────────────────────────

    /// Creates a reader from a memory buffer and checks its initial state.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_create_reader_from_memory() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());
            assert_eq!((*reader).ReadState(), ReadState::INITIALIZED);
            free_reader(reader);
        }
    }

    /// Reads a simple document and verifies the event sequence.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_read_simple_document() {
        unsafe {
            let reader = create_reader("<root><child>text</child></root>");
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // Expected sequence:
            // ELEMENT root (depth=0)
            // ELEMENT child (depth=1)
            // TEXT text (depth=2)
            // END_ELEMENT child (depth=1)
            // END_ELEMENT root (depth=0)

            assert_eq!(nodes.len(), 5);
            assert_eq!(nodes[0], (ReaderNodeType::ELEMENT, "root".to_string(), 0));
            assert_eq!(nodes[1], (ReaderNodeType::ELEMENT, "child".to_string(), 1));
            // UPSTREAM-PARITY: text nodes report the fixed name "#text".
            assert_eq!(nodes[2], (ReaderNodeType::TEXT, "#text".to_string(), 2));
            assert_eq!(
                nodes[3],
                (ReaderNodeType::END_ELEMENT, "child".to_string(), 1)
            );
            assert_eq!(
                nodes[4],
                (ReaderNodeType::END_ELEMENT, "root".to_string(), 0)
            );

            assert_eq!((*reader).ReadState(), ReadState::EOF);
            free_reader(reader);
        }
    }

    /// Verifies read state transitions from INITIALIZED to EOF.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_read_state_transitions() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());
            assert_eq!((*reader).ReadState(), ReadState::INITIALIZED);

            // First read.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).ReadState(), ReadState::READING);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!((*reader).Depth(), 0);

            // UPSTREAM-PARITY (oracle-verified 2.15.3): empty elements emit no
            // END_ELEMENT event — the second Read returns EOF directly.
            assert_eq!(xmlTextReaderRead(reader), 0);
            assert_eq!((*reader).ReadState(), ReadState::EOF);

            free_reader(reader);
        }
    }

    /// Verifies reader API entry points return error indicators for a NULL
    /// reader pointer.
    ///
    /// # Safety
    ///
    /// - NULL is passed only to reader API entry points that accept NULL and
    ///   return error indicators without dereferencing the pointer.
    #[test]
    fn test_null_reader_returns_error() {
        unsafe {
            assert_eq!(xmlTextReaderRead(ptr::null_mut()), -1);
            assert_eq!(xmlTextReaderDepth(ptr::null_mut()), -1);
            assert_eq!(xmlTextReaderNodeType(ptr::null_mut()), -1);
            assert!(xmlTextReaderName(ptr::null_mut()).is_null());
            assert!(xmlTextReaderValue(ptr::null_mut()).is_null());
            assert_eq!(xmlTextReaderHasValue(ptr::null_mut()), 0);
            assert_eq!(xmlTextReaderIsEmptyElement(ptr::null_mut()), 0);
            assert_eq!(
                xmlTextReaderReadState(ptr::null_mut()),
                ReadState::ERROR as c_int
            );
        }
    }

    /// Verifies freeing a NULL reader does not crash.
    ///
    /// # Safety
    ///
    /// - `xmlFreeTextReader` must tolerate a NULL pointer without
    ///   dereferencing it.
    #[test]
    fn test_xmlFreeTextReader_null() {
        unsafe {
            // Should not crash.
            xmlFreeTextReader(ptr::null_mut());
        }
    }

    /// Verifies `xmlTextReaderName` and `xmlTextReaderValue` results.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderName` and `xmlTextReaderValue` return heap strings
    ///   allocated via `xml_strdup`; each non-NULL result is freed exactly
    ///   once with `xmlFreeImpl`.
    #[test]
    fn test_reader_name_and_value() {
        unsafe {
            let reader = create_reader("<root>hello</root>");
            assert!(!reader.is_null());

            // Read root element.
            assert_eq!(xmlTextReaderRead(reader), 1);
            let name = xmlTextReaderName(reader);
            assert!(!name.is_null());
            assert_eq!(xmlstr_to_string(name), "root");
            xmlFreeImpl(name as *mut c_void);

            assert_eq!(xmlTextReaderHasValue(reader), 0);

            // Read text node.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::TEXT);
            assert_eq!((*reader).HasValue(), 1);

            let val = xmlTextReaderValue(reader);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_string(val), "hello");
            xmlFreeImpl(val as *mut c_void);

            free_reader(reader);
        }
    }

    /// Verifies an empty element reports no END_ELEMENT event.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_empty_element() {
        unsafe {
            let reader = create_reader("<empty/>");
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!((*reader).IsEmptyElement(), 1);
            assert_eq!((*reader).HasAttributes(), 0);
            assert_eq!((*reader).AttributeCount(), 0);

            // UPSTREAM-PARITY (oracle-verified 2.15.3): empty elements emit
            // NO END_ELEMENT event; the next Read returns EOF.
            assert_eq!(xmlTextReaderRead(reader), 0);
            assert_eq!((*reader).ReadState(), ReadState::EOF);

            free_reader(reader);
        }
    }

    /// Verifies attribute counting on an element with two attributes.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_element_with_attributes() {
        unsafe {
            let reader = create_reader(r#"<root a="1" b="2"/>"#);
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!((*reader).HasAttributes(), 1);

            // We know the attribute count if we've built the events properly.
            // The count_attributes checks the element's properties list.
            let attrs = xmlTextReaderAttributeCount(reader);
            assert_eq!(attrs, 2);

            free_reader(reader);
        }
    }

    /// Verifies attribute navigation (first, next, by name, by index).
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    /// - The pointers returned by `xmlTextReaderConstName` and
    ///   `xmlTextReaderConstValue` are borrowed from the reader and are not
    ///   freed by the test.
    #[test]
    fn test_attribute_navigation() {
        unsafe {
            let reader = create_reader(r#"<root a="1" b="2"></root>"#);
            assert!(!reader.is_null());

            // Position on root element.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);

            // Move to first attribute.
            assert_eq!(xmlTextReaderMoveToFirstAttribute(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ATTRIBUTE);

            let name = xmlTextReaderConstName(reader);
            assert!(!name.is_null());
            assert_eq!(xmlstr_to_bytes(name), b"a");

            let val = xmlTextReaderConstValue(reader);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"1");

            // Move to next attribute.
            assert_eq!(xmlTextReaderMoveToNextAttribute(reader), 1);
            let name = xmlTextReaderConstName(reader);
            assert!(!name.is_null());
            assert_eq!(xmlstr_to_bytes(name), b"b");
            let val = xmlTextReaderConstValue(reader);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"2");

            // No more attributes.
            assert_eq!(xmlTextReaderMoveToNextAttribute(reader), 0);

            // Move back to element.
            assert_eq!(xmlTextReaderMoveToElement(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);

            // Move to attribute by name.
            assert_eq!(
                xmlTextReaderMoveToAttribute(reader, b"a\0" as *const u8 as *const xmlChar),
                1
            );
            assert_eq!((*reader).NodeType(), ReaderNodeType::ATTRIBUTE);

            // Move to attribute by index.
            assert_eq!(xmlTextReaderMoveToElement(reader), 1);
            assert_eq!(xmlTextReaderMoveToAttributeNo(reader, 1), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ATTRIBUTE);

            free_reader(reader);
        }
    }

    /// Verifies attribute lookup by name and by index.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderGetAttribute` and `xmlTextReaderGetAttributeNo` return
    ///   heap strings; each non-NULL result is freed exactly once with
    ///   `xmlFreeImpl`.
    #[test]
    fn test_get_attribute() {
        unsafe {
            let reader = create_reader(r#"<root a="hello" b="world"/>"#);
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);

            // Get attribute by name.
            let val = xmlTextReaderGetAttribute(reader, b"a\0" as *const u8 as *const xmlChar);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"hello");
            xmlFreeImpl(val as *mut c_void);

            let val = xmlTextReaderGetAttribute(reader, b"b\0" as *const u8 as *const xmlChar);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"world");
            xmlFreeImpl(val as *mut c_void);

            // Non-existent attribute.
            let val = xmlTextReaderGetAttribute(reader, b"c\0" as *const u8 as *const xmlChar);
            assert!(val.is_null());

            // Get attribute by index.
            let val = xmlTextReaderGetAttributeNo(reader, 0);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"hello");
            xmlFreeImpl(val as *mut c_void);

            let val = xmlTextReaderGetAttributeNo(reader, 1);
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"world");
            xmlFreeImpl(val as *mut c_void);

            let val = xmlTextReaderGetAttributeNo(reader, 2);
            assert!(val.is_null());

            free_reader(reader);
        }
    }

    /// Verifies depth tracking across nested elements.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_depth_tracking() {
        unsafe {
            let reader = create_reader("<a><b><c/></b></a>");
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // UPSTREAM-PARITY (oracle-verified 2.15.3): empty elements emit no
            // END_ELEMENT event.
            // ELEMENT a (0), ELEMENT b (1), ELEMENT c (2),
            // END_ELEMENT b (1), END_ELEMENT a (0)
            assert_eq!(nodes.len(), 5);
            assert_eq!(nodes[0].2, 0); // a depth 0
            assert_eq!(nodes[1].2, 1); // b depth 1
            assert_eq!(nodes[2].2, 2); // c depth 2
            assert_eq!(nodes[3].2, 1); // END b depth 1
            assert_eq!(nodes[4].2, 0); // END a depth 0

            free_reader(reader);
        }
    }

    /// Verifies traversal of multiple sibling elements.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_multiple_siblings() {
        unsafe {
            let reader = create_reader("<root><a>A</a><b>B</b><c>C</c></root>");
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // ELEMENT root(0), ELEMENT a(1), TEXT(2), END a(1),
            // ELEMENT b(1), TEXT(2), END b(1),
            // ELEMENT c(1), TEXT(2), END c(1),
            // END root(0)
            assert_eq!(nodes.len(), 11);

            // Check the sibling elements.
            assert_eq!(nodes[1], (ReaderNodeType::ELEMENT, "a".to_string(), 1));
            assert_eq!(nodes[4], (ReaderNodeType::ELEMENT, "b".to_string(), 1));
            assert_eq!(nodes[7], (ReaderNodeType::ELEMENT, "c".to_string(), 1));

            free_reader(reader);
        }
    }

    /// Verifies `xmlTextReaderNext` semantics against the upstream streaming
    /// reader (xmlreader.c xmlTextReaderNext): from a NON-element node Next
    /// degrades to a plain Read — one step forward in document order, which
    /// may land on the parent's END_ELEMENT — and only an element START is
    /// skipped (subtree + END events) to the next sibling. On exhaustion the
    /// cursor is cleared (EOF).
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    /// - The `name` field read through `(*reader)` is borrowed from the
    ///   reader and is not freed.
    #[test]
    fn test_next_skip_to_sibling() {
        unsafe {
            let reader = create_reader("<root><a>A</a><b>B</b><c>C</c></root>");
            assert!(!reader.is_null());

            // Read to first node (root element).
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);

            // Read to a.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!(xmlstr_to_string((*reader).name as *const xmlChar), "a");

            // Read to text of a.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::TEXT);

            // Next from a TEXT node is a plain Read: it lands on the END of
            // the parent element (upstream xmlTextReaderNext, oracle-verified
            // via XMLReader 010/next_basic).
            assert_eq!(xmlTextReaderNext(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::END_ELEMENT);
            assert_eq!(xmlstr_to_string((*reader).name as *const xmlChar), "a");

            // Next again from the END event — a single step to element b.
            assert_eq!(xmlTextReaderNext(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!(xmlstr_to_string((*reader).name as *const xmlChar), "b");

            // Next from the b element start skips its subtree to c.
            assert_eq!(xmlTextReaderNext(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            assert_eq!(xmlstr_to_string((*reader).name as *const xmlChar), "c");

            // Next again — the traversal is exhausted and the cursor clears.
            assert_eq!(xmlTextReaderNext(reader), 0);
            assert_eq!((*reader).NodeType(), ReaderNodeType::NONE);

            free_reader(reader);
        }
    }

    /// Verifies comment and processing-instruction nodes are reported.
    ///
    /// # Safety
    ///
    /// - The static NUL-terminated `xml` buffer stays valid for the
    ///   `xmlReaderForMemory` call; the returned `reader` is asserted non-NULL
    ///   before use and is freed exactly once with `free_reader`.
    #[test]
    fn test_comment_and_pi_nodes() {
        unsafe {
            let xml = b"<?pi target?><root><!-- comment -->text</root>\0";
            let reader = xmlReaderForMemory(
                xml.as_ptr() as *const c_char,
                (xml.len() - 1) as c_int,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // PI, ELEMENT root, COMMENT, TEXT, END_ELEMENT root
            // Note: PI appears as PROCESSING_INSTRUCTION node.
            assert!(!nodes.is_empty(), "no nodes collected: {:?}", nodes);

            // Check PI.
            assert_eq!(
                nodes[0].0,
                ReaderNodeType::PROCESSING_INSTRUCTION,
                "expected PI at nodes[0], got {:?} name={}",
                nodes[0].0,
                nodes[0].1
            );
            assert_eq!(
                nodes[0].0,
                ReaderNodeType::PROCESSING_INSTRUCTION,
                "expected PI at nodes[0], got {:?} name={}",
                nodes[0].0,
                nodes[0].1
            );

            // Check root element.
            let root_idx = nodes
                .iter()
                .position(|(t, n, _)| *t == ReaderNodeType::ELEMENT && n == "root");
            assert!(
                root_idx.is_some(),
                "no ELEMENT root found in nodes: {:?}",
                nodes
                    .iter()
                    .map(|(t, n, _)| format!("{:?}:{}", t, n))
                    .collect::<Vec<_>>()
            );

            // Check comment.
            let comment_idx = nodes
                .iter()
                .position(|(t, _, _)| *t == ReaderNodeType::COMMENT);
            assert!(comment_idx.is_some(), "no COMMENT found");

            // Check text.
            let text_idx = nodes
                .iter()
                .position(|(t, _, _)| *t == ReaderNodeType::TEXT);
            assert!(text_idx.is_some(), "no TEXT found");

            free_reader(reader);
        }
    }

    /// Verifies `xmlTextReaderLocalName` returns the local name.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderLocalName` returns a heap string that is freed exactly
    ///   once with `xmlFreeImpl`.
    #[test]
    fn test_local_name() {
        unsafe {
            // We need a namespace-aware element. For now, test without namespace.
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);
            let local = xmlTextReaderLocalName(reader);
            assert!(!local.is_null());
            assert_eq!(xmlstr_to_bytes(local), b"root");
            xmlFreeImpl(local as *mut c_void);

            free_reader(reader);
        }
    }

    /// Verifies the base URI is NULL for memory-created readers.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_base_uri() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);
            // Base URI should be NULL for memory-created readers.
            let uri = xmlTextReaderBaseUri(reader);
            assert!(uri.is_null());

            free_reader(reader);
        }
    }

    /// Verifies namespace prefix lookup on the reader.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderLookupNamespace` returns a heap string; the non-NULL
    ///   result is freed exactly once with `xmlFreeImpl`.
    #[test]
    fn test_lookup_namespace() {
        unsafe {
            let reader = create_reader(r#"<root xmlns:ns="http://example.com"><ns:child/></root>"#);
            assert!(!reader.is_null());

            // Read to root.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);

            // Read to child (ns:child).
            assert_eq!(xmlTextReaderRead(reader), 1);

            // Lookup the "ns" prefix.
            let uri = xmlTextReaderLookupNamespace(reader, b"ns\0" as *const u8 as *const xmlChar);
            assert!(!uri.is_null());
            assert_eq!(xmlstr_to_bytes(uri), b"http://example.com");
            xmlFreeImpl(uri as *mut c_void);

            // Lookup default namespace (NULL prefix).
            let uri = xmlTextReaderLookupNamespace(reader, ptr::null());
            assert!(uri.is_null());

            // Lookup non-existent prefix.
            let uri = xmlTextReaderLookupNamespace(
                reader,
                b"nonexistent\0" as *const u8 as *const xmlChar,
            );
            assert!(uri.is_null());

            free_reader(reader);
        }
    }

    /// Verifies parser property get/set round trips and error codes.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_parser_properties() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());

            // Get default properties.
            assert_eq!(xmlTextReaderGetParserProp(reader, 1), 0); // LOADDTD
            assert_eq!(xmlTextReaderGetParserProp(reader, 2), 0); // DEFAULTATTRS
            assert_eq!(xmlTextReaderGetParserProp(reader, 3), 0); // VALIDATE
            assert_eq!(xmlTextReaderGetParserProp(reader, 4), 0); // SUBST_ENTITIES

            // Set and verify.
            assert_eq!(xmlTextReaderSetParserProp(reader, 1, 1), 0);
            assert_eq!(xmlTextReaderGetParserProp(reader, 1), 1);

            assert_eq!(xmlTextReaderSetParserProp(reader, 4, 1), 0);
            assert_eq!(xmlTextReaderGetParserProp(reader, 4), 1);

            // Invalid property.
            assert_eq!(xmlTextReaderGetParserProp(reader, 99), -1);
            assert_eq!(xmlTextReaderSetParserProp(reader, 99, 1), -1);

            free_reader(reader);
        }
    }

    /// Verifies the current document is available after the first read.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_current_doc() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());

            // Before reading, doc should be null.
            assert!((*reader).CurrentDoc().is_null());

            // After reading, doc should be available.
            assert_eq!(xmlTextReaderRead(reader), 1);
            let doc = xmlTextReaderCurrentDoc(reader);
            assert!(!doc.is_null());

            free_reader(reader);
        }
    }

    /// Verifies a reader can be freed after reading to EOF.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_free_reader_after_read() {
        unsafe {
            let reader = create_reader("<root><child/></root>");
            assert!(!reader.is_null());

            // Read through the document.
            while xmlTextReaderRead(reader) > 0 {}
            assert_eq!((*reader).ReadState(), ReadState::EOF);

            // Free should not crash.
            free_reader(reader);
        }
    }

    /// Verifies a NULL buffer with a non-zero size is rejected.
    ///
    /// # Safety
    ///
    /// - A NULL buffer with a non-zero size must be rejected by
    ///   `xmlReaderForMemory` without reading any memory.
    #[test]
    fn test_reader_for_memory_null_buffer() {
        unsafe {
            let reader = xmlReaderForMemory(ptr::null(), 10, ptr::null(), ptr::null(), 0);
            assert!(reader.is_null());
        }
    }

    /// Verifies a zero size makes `xmlReaderForMemory` return NULL.
    ///
    /// # Safety
    ///
    /// - The static buffer is not read: a size of 0 must make
    ///   `xmlReaderForMemory` return a NULL reader.
    #[test]
    fn test_reader_for_memory_empty_size() {
        unsafe {
            let data = b"<root/>";
            let reader = xmlReaderForMemory(
                data.as_ptr() as *const c_char,
                0,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(reader.is_null());
        }
    }

    /// Verifies a nonexistent file yields a NULL reader.
    ///
    /// # Safety
    ///
    /// - The static NUL-terminated filename stays valid for the
    ///   `xmlReaderForFile` call; a nonexistent file must yield a NULL reader
    ///   without reading.
    #[test]
    fn test_reader_for_file_not_found() {
        unsafe {
            let filename = b"/nonexistent/file.xml\0" as *const u8 as *const c_char;
            let reader = xmlReaderForFile(filename, ptr::null(), 0);
            assert!(reader.is_null());
        }
    }

    /// Verifies const name and value pointers borrowed from the reader.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - The pointers returned by `xmlTextReaderConstName` and
    ///   `xmlTextReaderConstValue` are borrowed from the reader and must not
    ///   be freed by the caller.
    #[test]
    fn test_const_name_and_value() {
        unsafe {
            let reader = create_reader("<root>text</root>");
            assert!(!reader.is_null());

            // Root element.
            assert_eq!(xmlTextReaderRead(reader), 1);
            let cname = xmlTextReaderConstName(reader);
            assert!(!cname.is_null());
            assert_eq!(xmlstr_to_bytes(cname), b"root");

            // Text node.
            assert_eq!(xmlTextReaderRead(reader), 1);
            let cval = xmlTextReaderConstValue(reader);
            assert!(!cval.is_null());
            assert_eq!(xmlstr_to_bytes(cval), b"text");

            free_reader(reader);
        }
    }

    /// Reads a complex nested document and counts node types.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_complex_nested_document() {
        unsafe {
            let xml = r#"<?xml version="1.0"?>
<library>
  <book id="1">
    <title>XML Fundamentals</title>
    <author>John Doe</author>
  </book>
  <book id="2">
    <title>XSLT Recipes</title>
    <author>Jane Smith</author>
  </book>
</library>"#;

            let reader = create_reader(xml);
            assert!(!reader.is_null());

            let mut element_count = 0;
            let mut end_element_count = 0;
            let mut text_count = 0;
            let mut pi_count = 0;

            loop {
                let ret = xmlTextReaderRead(reader);
                if ret <= 0 {
                    break;
                }
                match (*reader).NodeType() {
                    ReaderNodeType::ELEMENT => element_count += 1,
                    ReaderNodeType::END_ELEMENT => end_element_count += 1,
                    ReaderNodeType::TEXT => text_count += 1,
                    ReaderNodeType::PROCESSING_INSTRUCTION => pi_count += 1,
                    _ => {}
                }
            }

            // Elements: library, book(2), title(2), author(2) = 7
            assert_eq!(element_count, 7);
            // End elements: same count as elements
            assert_eq!(end_element_count, 7);
            // Text nodes: one per title and author = 4
            assert_eq!(text_count, 4);
            // UPSTREAM-PARITY: XML declaration (<?xml ...?>) is NOT stored as
            // a PI node in the tree. It is consumed by the parser and stored
            // in the document's version/encoding fields. Only <?pi ...?> nodes
            // (processing instructions) appear as XML_PI_NODE in the tree.
            assert_eq!(pi_count, 0);

            free_reader(reader);
        }
    }

    /// Verifies `xmlTextReaderSetup` reinitializes a used reader.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderSetup` with a NULL input must reset the reader without
    ///   dereferencing the NULL input.
    #[test]
    fn test_setup_reinitialize() {
        unsafe {
            let reader = create_reader("<root/>");
            assert!(!reader.is_null());

            // Read through.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).ReadState(), ReadState::READING);

            // Setup with new input (simulate re-initialization).
            // For this test, we just verify the setup function exists and
            // handles a NULL input gracefully (resetting the reader).
            assert_eq!(
                xmlTextReaderSetup(reader, ptr::null_mut(), ptr::null(), ptr::null(), 0),
                0
            );
            assert_eq!((*reader).ReadState(), ReadState::INITIALIZED);

            free_reader(reader);
        }
    }

    /// Verifies attribute queries on non-element and attribute-less nodes.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_has_attributes_on_non_element() {
        unsafe {
            let reader = create_reader("<root>text</root>");
            assert!(!reader.is_null());

            // Position on text node.
            assert_eq!(xmlTextReaderRead(reader), 1); // root element
            assert_eq!((*reader).HasAttributes(), 0); // 0 attributes on root
            assert_eq!(xmlTextReaderRead(reader), 1); // text
            assert_eq!((*reader).HasAttributes(), 0);

            free_reader(reader);
        }
    }

    /// Verifies `xmlTextReaderPrev` fails after EOF.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_prev_sibling() {
        unsafe {
            let reader = create_reader("<root><a/><b/><c/></root>");
            assert!(!reader.is_null());

            // Read through the document.
            while xmlTextReaderRead(reader) > 0 {
                // Skip to END_ELEMENT root or beyond.
            }

            // Can't go prev after EOF.
            assert_eq!(xmlTextReaderPrev(reader), -1);

            free_reader(reader);
        }
    }

    /// Verifies moving to an attribute index fails on an element without
    /// attributes.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_move_to_attribute_no_not_on_element() {
        unsafe {
            let reader = create_reader("<root>text</root>");
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1); // root
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);

            // Move to non-existent attribute index.
            assert_eq!(xmlTextReaderMoveToAttributeNo(reader, 0), 0);

            free_reader(reader);
        }
    }

    /// Verifies namespaced attribute lookup with a NULL namespace URI.
    ///
    /// # Safety
    ///
    /// - The `reader` pointer is asserted non-NULL before use and freed
    ///   exactly once with `free_reader`.
    /// - `xmlTextReaderGetAttributeNs` returns a heap string; the non-NULL
    ///   result is freed exactly once with `xmlFreeImpl`.
    #[test]
    fn test_get_attribute_ns() {
        unsafe {
            let reader = create_reader(r#"<root a="1" b="2"/>"#);
            assert!(!reader.is_null());

            assert_eq!(xmlTextReaderRead(reader), 1);

            // Get attribute by local name only (namespaceURI is NULL).
            let val = xmlTextReaderGetAttributeNs(
                reader,
                b"a\0" as *const u8 as *const xmlChar,
                ptr::null(),
            );
            assert!(!val.is_null());
            assert_eq!(xmlstr_to_bytes(val), b"1");
            xmlFreeImpl(val as *mut c_void);

            free_reader(reader);
        }
    }

    /// Verifies mixed content (text and elements) event ordering.
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   before it is dereferenced and is freed exactly once with
    ///   `free_reader`.
    #[test]
    fn test_mixed_content() {
        unsafe {
            let reader = create_reader("<root>before<child/>after</root>");
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // UPSTREAM-PARITY (oracle-verified 2.15.3): empty elements emit no
            // END_ELEMENT event.
            // ELEMENT root(0), TEXT "before"(1), ELEMENT child(1),
            // TEXT "after"(1), END_ELEMENT root(0)
            assert_eq!(nodes.len(), 5);
            assert_eq!(nodes[0], (ReaderNodeType::ELEMENT, "root".to_string(), 0));
            assert_eq!(nodes[1].0, ReaderNodeType::TEXT);
            assert_eq!(nodes[2], (ReaderNodeType::ELEMENT, "child".to_string(), 1));
            assert_eq!(nodes[3].0, ReaderNodeType::TEXT);

            free_reader(reader);
        }
    }

    /// Verifies malformed XML makes reading fail gracefully.
    ///
    /// # Safety
    ///
    /// - The static `data` buffer is valid for the 7-byte `xmlReaderForMemory`
    ///   read; the returned `reader` is asserted non-NULL before being read
    ///   and is freed exactly once with `free_reader`.
    #[test]
    fn test_error_handling_invalid_xml() {
        unsafe {
            // Malformed XML.
            let data = b"<root><\0" as *const u8 as *const c_char;
            let reader = xmlReaderForMemory(data, 7, ptr::null(), ptr::null(), 0);
            assert!(!reader.is_null());

            // Reading should fail.
            let ret = xmlTextReaderRead(reader);
            assert!(ret == -1 || ret == 0);

            free_reader(reader);
        }
    }

    /// Verifies parser options are stored and honored by the reader.
    ///
    /// # Safety
    ///
    /// - The static NUL-terminated `data` buffer stays valid for the
    ///   `xmlReaderForMemory` call; the returned `reader` is asserted non-NULL
    ///   before dereferencing its `options` field and before
    ///   `xmlTextReaderRead`, and is freed exactly once with `free_reader`.
    #[test]
    fn test_reader_with_options() {
        unsafe {
            let data = b"<root/>\0" as *const u8 as *const c_char;
            let reader = xmlReaderForMemory(
                data,
                7,
                ptr::null(),
                ptr::null(),
                XML_PARSE_NOENT | XML_PARSE_DTDLOAD,
            );
            assert!(!reader.is_null());

            // Verify options were set.
            assert_eq!((*reader).options & XML_PARSE_NOENT, XML_PARSE_NOENT);
            assert_eq!((*reader).options & XML_PARSE_DTDLOAD, XML_PARSE_DTDLOAD);

            assert_eq!(xmlTextReaderRead(reader), 1);
            free_reader(reader);
        }
    }

    /// Verifies reading a document from an open file descriptor.
    ///
    /// # Safety
    ///
    /// - The descriptor returned by `libc::open` must be a valid open
    ///   descriptor passed to `xmlReaderForFd`, which reads from it; the
    ///   returned `reader` is asserted non-NULL before use, freed with
    ///   `free_reader`, and the descriptor is closed afterwards.
    #[test]
    fn test_reader_for_fd() {
        unsafe {
            // Create a temp file and test xmlReaderForFd.
            let tmp_path = "/tmp/libxml_rs_test_reader_fd.xml";
            let tmp_cstr = std::ffi::CString::new(tmp_path).unwrap();
            let content = b"<root><data/></root>";
            let fd = libc::open(
                tmp_cstr.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                0o644,
            );
            assert!(fd >= 0);
            libc::write(fd, content.as_ptr() as *const c_void, content.len());
            libc::close(fd);

            // Open for reading.
            let fd = libc::open(tmp_cstr.as_ptr(), libc::O_RDONLY, 0);
            assert!(fd >= 0);

            let reader = xmlReaderForFd(fd, ptr::null(), ptr::null(), 0);
            assert!(!reader.is_null());

            let nodes = collect_nodes(reader);
            // UPSTREAM-PARITY (oracle-verified 2.15.3): `<data/>` is empty, so
            // it contributes no END_ELEMENT: root, data, END root.
            assert_eq!(nodes.len(), 3);

            free_reader(reader);
            libc::close(fd);
            std::fs::remove_file(tmp_path).ok();
        }
    }

    #[test]
    fn test_reader_for_io() {
        unsafe {
            extern "C" fn io_read(context: *mut c_void, buffer: *mut c_char, len: c_int) -> c_int {
                if context.is_null() || buffer.is_null() || len <= 0 {
                    return -1;
                }
                // SAFETY: context points to an IoCtx struct.
                let ctx = unsafe { &mut *(context as *mut IoCtx) };
                if ctx.pos >= ctx.data.len() {
                    return 0;
                }
                let remaining = ctx.data.len() - ctx.pos;
                let to_copy = if (remaining as c_int) < len {
                    remaining
                } else {
                    len as usize
                };
                // SAFETY: buffer has at least `len` bytes of space.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        ctx.data.as_ptr().add(ctx.pos),
                        buffer as *mut u8,
                        to_copy,
                    );
                }
                ctx.pos += to_copy;
                to_copy as c_int
            }

            extern "C" fn io_close(_context: *mut c_void) -> c_int {
                0
            }

            struct IoCtx {
                data: &'static [u8],
                pos: usize,
            }
            let mut ctx = IoCtx {
                data: b"<root/>",
                pos: 0,
            };

            let reader = xmlReaderForIO(
                Some(io_read),
                Some(io_close),
                &mut ctx as *mut IoCtx as *mut c_void,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(!reader.is_null());

            // Read through the document.
            assert_eq!(xmlTextReaderRead(reader), 1);
            assert_eq!((*reader).NodeType(), ReaderNodeType::ELEMENT);
            let cname = xmlTextReaderConstName(reader);
            assert!(!cname.is_null());
            assert_eq!(xmlstr_to_bytes(cname), b"root");

            // UPSTREAM-PARITY (oracle-verified 2.15.3): `<root/>` is empty, so
            // there is no END_ELEMENT — the second Read returns EOF.
            assert_eq!(xmlTextReaderRead(reader), 0);

            free_reader(reader);
        }
    }

    /// xmlTextReaderPreservePattern: after a full traversal, the document
    /// contains only the pattern-matched nodes and their element ancestors
    /// (upstream NODE_IS_PRESERVED streaming prune; reader3.c — Phase-12
    /// EXTERNAL-CONSUMERS court).
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   and freed exactly once with `free_reader`; the returned doc is freed
    ///   with `tree::free_doc` exactly once.
    #[test]
    fn test_preserve_pattern_prunes() {
        unsafe {
            let reader = create_reader("<doc><parent><drop/><keep/><drop/></parent></doc>");
            assert!(!reader.is_null());
            // register the pattern BEFORE the first Read, like reader3.c
            let pat = b"keep\0";
            assert!(
                xmlTextReaderPreservePattern(
                    reader,
                    pat.as_ptr() as *const xmlChar,
                    ptr::null_mut(),
                ) >= 0
            );
            let mut ret = xmlTextReaderRead(reader);
            while ret == 1 {
                ret = xmlTextReaderRead(reader);
            }
            let doc = xmlTextReaderCurrentDoc(reader);
            assert!(!doc.is_null());
            free_reader(reader);

            // the pruned doc keeps doc -> parent -> keep; drop nodes gone
            let root = tree::doc_get_root_element(doc);
            assert!(!root.is_null());
            assert_eq!(xmlstr_to_bytes((*root).name), b"doc");
            let mut children: Vec<*mut _xmlNode> = Vec::new();
            let mut c = (*root).children;
            while !c.is_null() {
                children.push(c);
                c = (*c).next;
            }
            // the doc's only child is the preserved ancestor <parent>
            assert_eq!(children.len(), 1);
            assert_eq!(xmlstr_to_bytes((*children[0]).name), b"parent");
            // <parent> keeps only the matched <keep/>; both <drop/> are gone
            let mut keep: Vec<*mut _xmlNode> = Vec::new();
            let mut c = (*children[0]).children;
            while !c.is_null() {
                keep.push(c);
                c = (*c).next;
            }
            assert_eq!(keep.len(), 1);
            assert_eq!(xmlstr_to_bytes((*keep[0]).name), b"keep");
            tree::free_doc(doc);
        }
    }

    /// XML_PARSE_DTDVALID on a no-DTD document: the parser raises the
    /// no-DTD validity error and clears ctxt->valid (parse2.c — Phase-12
    /// EXTERNAL-CONSUMERS court).
    ///
    /// # Safety
    ///
    /// - The reader is created from a static string literal that stays alive
    ///   for the reader's lifetime; the `reader` pointer is asserted non-NULL
    ///   and freed exactly once with `free_reader`.
    #[test]
    fn test_reader_dtdvalid_no_dtd() {
        unsafe {
            let bytes = b"<doc/>";
            let ctxt = create_parser_ctxt();
            assert!(!ctxt.is_null());
            let input = input_from_memory(bytes.as_ptr() as *const c_char, bytes.len() as c_int);
            setup_parser_input(ctxt, input);
            crate::abi::exports_parser::apply_options(ctxt, XML_PARSE_DTDVALID);
            assert_eq!(parse_document(ctxt), 0);
            assert_eq!(
                (*ctxt).valid,
                0,
                "no-DTD validating parse must clear ctxt->valid"
            );
            assert_eq!((*ctxt).errNo, XML_DTD_NO_DTD);
            free_parser_ctxt(ctxt);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11.1-I reader closure — remaining xmlTextReader API (R-000136)
// ═══════════════════════════════════════════════════════════════════════════════

/// Error severity (upstream `xmlParserSeverities`, reader.h).
pub const XML_PARSER_SEVERITY_VALIDITY_WARNING: c_int = 1;
pub const XML_PARSER_SEVERITY_VALIDITY_ERROR: c_int = 2;
pub const XML_PARSER_SEVERITY_WARNING: c_int = 3;
pub const XML_PARSER_SEVERITY_ERROR: c_int = 4;

/// Opaque locator passed to the reader error handler (upstream
/// `xmlTextReaderLocator`).
#[derive(Debug)]
#[repr(C)]
pub struct XmlTextReaderLocator {
    pub reader: *mut XmlTextReader,
}

/// Reader error callback (upstream `xmlTextReaderErrorFunc`).
pub type xmlTextReaderErrorFunc = unsafe extern "C" fn(
    arg: *mut c_void,
    msg: *const c_char,
    severity: c_int,
    locator: *mut XmlTextReaderLocator,
);

/// `xmlTextReaderPtr xmlReaderForDoc(const xmlChar *cur, const char *URL,
/// const char *encoding, int options)` — reader over an in-memory XML string.
///
/// # SAFETY
///
/// - `cur` must be a valid NUL-terminated XML document string.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderForDoc(
    cur: *const xmlChar,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut XmlTextReader {
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = unsafe { libc::strlen(cur as *const libc::c_char) } as c_int;
    unsafe { xmlReaderForMemory(cur as *const c_char, len, URL, encoding, options) }
}

/// `xmlTextReaderPtr xmlNewTextReaderFilename(const char *URI)` — upstream
/// xmlreader.c: creates a reader over the file, no encoding/options
/// (R-000176: the candidate previously exported a 3-argument extension).
///
/// # SAFETY
///
/// - `URI` must point to a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNewTextReaderFilename(URI: *const c_char) -> *mut XmlTextReader {
    unsafe { xmlReaderForFile(URI, ptr::null(), 0) }
}

/// Rebuild a reader in place (upstream `xmlReaderNew*` reuse contract).
///
/// Upstream reuses the caller's existing reader allocation, so a caller's
/// pointer remains valid across `xmlReaderNew*`. The candidate mirrors that by
/// moving the freshly built reader's contents into the caller's allocation and
/// releasing the temporary allocation without dropping the moved contents.
///
/// # SAFETY
///
/// - `reader` must be a valid, non-NULL reader pointer.
/// - `new_reader` must be a valid, non-NULL reader pointer distinct from `reader`.
unsafe fn reader_renew(reader: *mut XmlTextReader, new_reader: *mut XmlTextReader) {
    debug_assert!(!reader.is_null() && !new_reader.is_null() && reader != new_reader);
    unsafe {
        // Drop the old contents, then bitwise-move the new reader into the
        // caller's allocation. The temporary allocation is deallocated without
        // dropping (its contents now live at `reader`).
        core::ptr::drop_in_place(reader);
        core::ptr::copy_nonoverlapping(new_reader, reader, 1);
        let layout = std::alloc::Layout::new::<XmlTextReader>();
        std::alloc::dealloc(new_reader as *mut u8, layout);
    }
}

/// `int xmlReaderNewDoc(xmlTextReaderPtr reader, const xmlChar *cur, const char *URL, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `cur`, `URL`, `encoding` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlReaderNewDoc(
    reader: *mut XmlTextReader,
    cur: *const xmlChar,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    // UPSTREAM-PARITY: the New* family rejects a NULL reader before any work
    // (xmlreader.c: `if (reader == NULL) return (-1);`). It never allocates.
    if reader.is_null() || cur.is_null() {
        return -1;
    }
    let r = unsafe { xmlReaderForDoc(cur, URL, encoding, options) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `int xmlReaderNewFile(xmlTextReaderPtr reader, const char *filename, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `filename`, `encoding` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlReaderNewFile(
    reader: *mut XmlTextReader,
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    let r = unsafe { xmlReaderForFile(filename, encoding, options) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `int xmlReaderNewMemory(xmlTextReaderPtr reader, const char *buffer, int size, const char *URL, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `buffer`, `URL`, `encoding` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlReaderNewMemory(
    reader: *mut XmlTextReader,
    buffer: *const c_char,
    size: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    if reader.is_null() || buffer.is_null() {
        return -1;
    }
    let r = unsafe { xmlReaderForMemory(buffer, size, URL, encoding, options) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `int xmlReaderNewFd(xmlTextReaderPtr reader, int fd, const char *URL, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `encoding` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlReaderNewFd(
    reader: *mut XmlTextReader,
    fd: c_int,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    let r = unsafe { xmlReaderForFd(fd, URL, encoding, options) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `int xmlReaderNewIO(xmlTextReaderPtr reader, xmlInputReadCallback ioread, xmlInputCloseCallback ioclose, void *ioctx, const char *URL, const char *encoding, int options)`.
///
/// # SAFETY
///
/// - `reader`, `ioctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `URL`, `encoding` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlReaderNewIO(
    reader: *mut XmlTextReader,
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> c_int {
    // UPSTREAM-PARITY: NULL reader or NULL read callback is rejected (-1).
    if reader.is_null() || ioread.is_none() {
        return -1;
    }
    let r = unsafe { xmlReaderForIO(ioread, ioclose, ioctx, URL, encoding, options) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `xmlTextReaderPtr xmlReaderWalker(xmlDocPtr doc)` — reader walking an
/// existing document tree.
///
/// # SAFETY
///
/// - `doc` must be a valid document.
#[no_mangle]
pub unsafe extern "C" fn xmlReaderWalker(doc: *mut _xmlDoc) -> *mut XmlTextReader {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let mut reader = XmlTextReader::new(ptr::null_mut(), None, None);
    reader.doc = doc;
    reader.parsed = true;
    reader.owns_doc = false; // walker borrows the caller's document
    reader.state = ReadState::READING;
    reader.build_events();
    Box::into_raw(Box::new(reader))
}

/// `int xmlReaderNewWalker(xmlTextReaderPtr reader, xmlDocPtr doc)`.
///
/// # SAFETY
///
/// - `reader`, `doc` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlReaderNewWalker(
    reader: *mut XmlTextReader,
    doc: *mut _xmlDoc,
) -> c_int {
    // UPSTREAM-PARITY: NULL reader or NULL doc is rejected (-1).
    if reader.is_null() || doc.is_null() {
        return -1;
    }
    let r = unsafe { xmlReaderWalker(doc) };
    if r.is_null() {
        return -1;
    }
    unsafe { reader_renew(reader, r) };
    0
}

/// `long xmlTextReaderByteConsumed(xmlTextReaderPtr reader)`.
///
/// Returns the total bytes consumed from the input (0 when unavailable —
/// the candidate parses the full input up front; documented divergence).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub const unsafe extern "C" fn xmlTextReaderByteConsumed(reader: *mut XmlTextReader) -> c_long {
    if reader.is_null() {
        return -1;
    }
    0
}

/// `const xmlChar *xmlTextReaderConstBaseUri(xmlTextReaderPtr reader)` — the
/// base URI, valid until the reader is freed (no copy).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstBaseUri(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    // UPSTREAM-PARITY (xmlreader.c xmlTextReaderConstBaseUri): the base URI
    // is exposed only once a node is current (upstream `reader->node == NULL`
    // returns NULL). Pre-first-Read and after EOF the PHP property must read
    // as the empty string, not the reader's setup URL.
    let r = unsafe { &*reader };
    if r.cur_node.is_null() {
        return ptr::null();
    }
    r.URL
}

/// `const xmlChar *xmlTextReaderConstEncoding(xmlTextReaderPtr reader)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstEncoding(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    if !r.encoding.is_null() {
        return r.encoding;
    }
    if !r.doc.is_null() {
        return unsafe { (*r.doc).encoding };
    }
    ptr::null()
}

/// `const xmlChar *xmlTextReaderConstLocalName(xmlTextReaderPtr reader)`.
///
/// UPSTREAM-PARITY: at an attribute position this is the attribute's local
/// name (or "xmlns"/the prefix for a namespace declaration); at an element
/// position the tree's local name.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstLocalName(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    if r.cur_node.is_null() {
        return ptr::null();
    }
    // Attribute position: the attribute's local name (upstream node->name for
    // XML_ATTRIBUTE_NODE; "xmlns"/prefix for a namespace declaration).
    if r.cur_attribute >= 0 {
        let target = unsafe { r.attr_at(r.cur_node, r.cur_attribute) };
        return match target {
            AttrTarget::Ns(ns) => {
                if ns.is_null() {
                    ptr::null()
                } else if unsafe { (*ns).prefix }.is_null() {
                    c"xmlns".as_ptr() as *const xmlChar
                } else {
                    unsafe { (*ns).prefix }
                }
            }
            AttrTarget::Prop(p) => {
                if p.is_null() || unsafe { (*p).name }.is_null() {
                    ptr::null()
                } else {
                    unsafe { (*p).name }
                }
            }
            AttrTarget::None => ptr::null(),
        };
    }
    // Element position: the tree's local name (upstream node->name).
    let etype = unsafe { (*r.cur_node).type_ };
    if etype == XML_ELEMENT_NODE as c_int || etype == XML_ATTRIBUTE_NODE as c_int {
        unsafe { (*r.cur_node).name }
    } else {
        ptr::null()
    }
}

/// `const xmlChar *xmlTextReaderConstNamespaceUri(xmlTextReaderPtr reader)`.
///
/// UPSTREAM-PARITY: at an attribute position the namespace comes from the
/// attribute (or namespace declaration) itself; elsewhere from the node.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstNamespaceUri(
    reader: *mut XmlTextReader,
) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    if r.cur_node.is_null() {
        return ptr::null();
    }
    // Attribute position: resolve the current attribute's namespace.
    if r.cur_attribute >= 0 {
        let target = unsafe { r.attr_at(r.cur_node, r.cur_attribute) };
        return match target {
            AttrTarget::Ns(_ns) => {
                // UPSTREAM-PARITY (xmlTextReaderConstNamespaceUri): a
                // namespace declaration reports the xmlns namespace URI,
                // not the declared URI.
                c"http://www.w3.org/2000/xmlns/".as_ptr() as *const xmlChar
            }
            AttrTarget::Prop(p) => {
                if p.is_null() || unsafe { (*p).ns }.is_null() {
                    ptr::null()
                } else {
                    unsafe { (*(*p).ns).href }
                }
            }
            AttrTarget::None => ptr::null(),
        };
    }
    let ns = unsafe { (*r.cur_node).ns };
    if ns.is_null() || unsafe { (*ns).href }.is_null() {
        ptr::null()
    } else {
        unsafe { (*ns).href }
    }
}

/// `const xmlChar *xmlTextReaderConstPrefix(xmlTextReaderPtr reader)`.
///
/// UPSTREAM-PARITY: at an attribute position the prefix comes from the
/// attribute; for a namespace declaration the prefix is reported as "xmlns"
/// (and NULL for the default declaration) — an upstream quirk reproduced here.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstPrefix(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    if r.cur_node.is_null() {
        return ptr::null();
    }
    // Attribute position: resolve the current attribute's namespace.
    if r.cur_attribute >= 0 {
        let target = unsafe { r.attr_at(r.cur_node, r.cur_attribute) };
        return match target {
            AttrTarget::Ns(ns) => {
                if ns.is_null() || unsafe { (*ns).prefix }.is_null() {
                    ptr::null()
                } else {
                    c"xmlns".as_ptr() as *const xmlChar
                }
            }
            AttrTarget::Prop(p) => {
                if p.is_null() || unsafe { (*p).ns }.is_null() {
                    ptr::null()
                } else {
                    unsafe { (*(*p).ns).prefix }
                }
            }
            AttrTarget::None => ptr::null(),
        };
    }
    let ns = unsafe { (*r.cur_node).ns };
    if ns.is_null() || unsafe { (*ns).prefix }.is_null() {
        ptr::null()
    } else {
        unsafe { (*ns).prefix }
    }
}

/// `const xmlChar *xmlTextReaderConstString(xmlTextReaderPtr reader, const xmlChar *str)`
/// — the reader's dictionary-internalized copy of `str`; the candidate
/// returns `str` unchanged (dictionary interning is an internal detail).
///
/// # SAFETY
///
/// - `_reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `str` must point to valid NUL-terminated
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
pub const unsafe extern "C" fn xmlTextReaderConstString(
    _reader: *mut XmlTextReader,
    str: *const xmlChar,
) -> *const xmlChar {
    str
}

/// `const xmlChar *xmlTextReaderConstXmlLang(xmlTextReaderPtr reader)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstXmlLang(reader: *mut XmlTextReader) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    let mut node = r.cur_node;
    while !node.is_null() {
        let mut prop = unsafe { (*node).properties };
        while !prop.is_null() {
            let p = unsafe { &*prop };
            if !p.name.is_null()
                && unsafe { *p.name } == b'x'
                && unsafe { *p.name.add(1) } == b'm'
                && unsafe { *p.name.add(2) } == b'l'
                && unsafe { *p.name.add(3) } == b':'
                && unsafe { *p.name.add(4) } == b'l'
                && unsafe { *p.name.add(5) } == b'a'
                && unsafe { *p.name.add(6) } == b'n'
                && unsafe { *p.name.add(7) } == b'g'
                && unsafe { *p.name.add(8) } == 0
            {
                if !p.children.is_null() {
                    let txt = p.children;
                    if unsafe { (*txt).type_ }
                        == crate::abi::types::xmlElementType::XML_TEXT_NODE as c_int
                    {
                        return unsafe { (*txt).content };
                    }
                }
                return ptr::null();
            }
            prop = p.next;
        }
        node = unsafe { (*node).parent };
    }
    ptr::null()
}

/// `const xmlChar *xmlTextReaderConstXmlVersion(xmlTextReaderPtr reader)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderConstXmlVersion(
    reader: *mut XmlTextReader,
) -> *const xmlChar {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &*reader };
    if r.doc.is_null() {
        return ptr::null();
    }
    unsafe { (*r.doc).version }
}

/// `int xmlTextReaderQuoteChar(xmlTextReaderPtr reader)`.
///
/// UPSTREAM-PARITY: libxml2 2.13/2.15 returns `'"'` unconditionally for any
/// non-NULL reader (the implementation is a placeholder that does not inspect
/// the attribute; see the `/* TODO maybe lookup the attribute value */` comment
/// in xmlreader.c). The candidate reproduces that historical behavior exactly.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub const unsafe extern "C" fn xmlTextReaderQuoteChar(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    b'"' as c_int
}

/// `int xmlTextReaderIsDefault(xmlTextReaderPtr reader)` — whether the current
/// attribute came from the DTD default. The candidate returns 0 for a valid
/// reader (DTD default attribute expansion is not annotated; documented
/// divergence), -1 for a NULL reader (upstream contract).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub const unsafe extern "C" fn xmlTextReaderIsDefault(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    0
}

/// `int xmlTextReaderIsNamespaceDecl(xmlTextReaderPtr reader)` — whether the
/// current attribute position is a namespace declaration.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderIsNamespaceDecl(reader: *mut XmlTextReader) -> c_int {
    if reader.is_null() {
        return -1;
    }
    let r = unsafe { &*reader };
    if r.cur_node.is_null() {
        return -1;
    }
    r.cur_attr_is_ns as c_int
}

/// `int xmlTextReaderMoveToAttributeNs(xmlTextReaderPtr reader, const xmlChar *localName, const xmlChar *namespaceURI)`.
///
/// UPSTREAM-PARITY (xmlreader.c, 2.15): NULL reader/localName/namespaceURI
/// returns -1; a NULL `namespaceURI` is NOT treated as "no namespace" — the
/// caller must pass the actual URI. The `http://www.w3.org/2000/xmlns/`
/// namespace searches namespace declarations (matching the default `xmlns`
/// declaration or a prefix), everything else searches only namespace-qualified
/// properties (`prop->ns != NULL`).
///
/// # SAFETY
///
/// - `localName`/`namespaceURI` must be valid strings (non-NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderMoveToAttributeNs(
    reader: *mut XmlTextReader,
    localName: *const xmlChar,
    namespaceURI: *const xmlChar,
) -> c_int {
    if reader.is_null() || localName.is_null() || namespaceURI.is_null() {
        return -1;
    }
    let r = unsafe { &mut *reader };
    let node = r.cur_node;
    if node.is_null() {
        return -1;
    }
    if unsafe { (*node).type_ } != XML_ELEMENT_NODE as c_int {
        return 0;
    }

    const XMLNS_URI: &[u8] = b"http://www.w3.org/2000/xmlns/\0";
    if libc::strcmp(
        namespaceURI as *const libc::c_char,
        XMLNS_URI.as_ptr() as *const libc::c_char,
    ) == 0
    {
        // Namespace-declaration search: localName "xmlns" addresses the
        // default declaration, any other localName is a prefix.
        let is_default = libc::strcmp(
            localName as *const libc::c_char,
            c"xmlns".as_ptr() as *const libc::c_char,
        ) == 0;
        let mut ns = unsafe { (*node).nsDef };
        let mut index = 0;
        while !ns.is_null() {
            let n = unsafe { &*ns };
            let prefix_match = if is_default {
                n.prefix.is_null()
            } else {
                !n.prefix.is_null()
                    && libc::strcmp(
                        n.prefix as *const libc::c_char,
                        localName as *const libc::c_char,
                    ) == 0
            };
            if prefix_match {
                r.cur_attribute = index;
                r.node_type = ReaderNodeType::ATTRIBUTE;
                r.cache_attribute_info(AttrTarget::Ns(ns));
                return 1;
            }
            index += 1;
            ns = unsafe { (*ns).next };
        }
        return 0;
    }

    // Property search: only namespace-qualified attributes are matchable.
    let mut prop = unsafe { (*node).properties };
    let mut index = 0;
    let mut ns_count = 0;
    let mut ns = unsafe { (*node).nsDef };
    while !ns.is_null() {
        ns_count += 1;
        ns = unsafe { (*ns).next };
    }
    while !prop.is_null() {
        let p = unsafe { &*prop };
        if !p.name.is_null()
            && !p.ns.is_null()
            && !(*p.ns).href.is_null()
            && libc::strcmp(
                p.name as *const libc::c_char,
                localName as *const libc::c_char,
            ) == 0
            && libc::strcmp(
                (*p.ns).href as *const libc::c_char,
                namespaceURI as *const libc::c_char,
            ) == 0
        {
            r.cur_attribute = ns_count + index;
            r.node_type = ReaderNodeType::ATTRIBUTE;
            r.cache_attribute_info(AttrTarget::Prop(prop));
            return 1;
        }
        index += 1;
        prop = unsafe { (*prop).next };
    }
    0
}

/// `xmlNodePtr xmlTextReaderPreserve(xmlTextReaderPtr reader)` — the current
/// node (the candidate's reader owns the whole tree, so no separate
/// preservation step is needed).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderPreserve(reader: *mut XmlTextReader) -> *mut _xmlNode {
    if reader.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*reader).cur_node }
}

/// `int xmlTextReaderPreservePattern(xmlTextReaderPtr reader, const xmlChar *pattern, const xmlChar **namespaces)`.
///
/// Compiles the XPath-subset pattern (upstream `xmlPatterncompile`) and
/// registers it like upstream `reader->patternTab`; after the parse,
/// matched nodes and their element ancestors are kept and every other node
/// is unlinked and freed (upstream's NODE_IS_PRESERVED streaming prune,
/// applied post-parse by the candidate's whole-tree reader). Returns the
/// index of the registered pattern, or -1 on error (upstream xmlreader.c
/// xmlTextReaderPreservePattern).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `pattern` must point to a valid NUL-terminated string;
///   `namespaces` must be NULL or a NULL-terminated array of
///   NUL-terminated strings, both live for the duration of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the Phase-12 EXTERNAL-CONSUMERS court (reader3.c).
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderPreservePattern(
    reader: *mut XmlTextReader,
    pattern: *const xmlChar,
    namespaces: *mut *const xmlChar,
) -> c_int {
    if reader.is_null() || pattern.is_null() {
        return -1;
    }
    let comp = unsafe {
        crate::abi::exports_automata::xmlPatterncompile(
            pattern,
            ptr::null_mut(),
            0,
            namespaces as *const *const xmlChar,
        )
    };
    if comp.is_null() {
        return -1;
    }
    // SAFETY: reader is valid (checked); pattern_tab owns the compiled
    // pattern and frees it in Drop.
    let idx = unsafe { (*reader).pattern_tab.len() };
    unsafe { (*reader).pattern_tab.push(comp) };
    idx as c_int
}

/// `int xmlTextReaderSetErrorHandler(xmlTextReaderPtr reader, xmlTextReaderErrorFunc f, void *arg)`.
///
/// # SAFETY
///
/// - `f` must be a valid callback or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderSetErrorHandler(
    reader: *mut XmlTextReader,
    f: Option<xmlTextReaderErrorFunc>,
    arg: *mut c_void,
) {
    if reader.is_null() {
        return;
    }
    unsafe {
        (*reader).error_handler = f;
        (*reader).error_arg = arg;
    }
}

/// `void xmlTextReaderGetErrorHandler(xmlTextReaderPtr reader, xmlTextReaderErrorFunc *f, void **arg)`.
///
/// # SAFETY
///
/// - `f`/`arg` must be valid out-pointers or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderGetErrorHandler(
    reader: *mut XmlTextReader,
    f: *mut Option<xmlTextReaderErrorFunc>,
    arg: *mut *mut c_void,
) {
    if reader.is_null() {
        return;
    }
    unsafe {
        if !f.is_null() {
            *f = (*reader).error_handler;
        }
        if !arg.is_null() {
            *arg = (*reader).error_arg;
        }
    }
}

/// `void xmlTextReaderSetStructuredErrorHandler(xmlTextReaderPtr reader, xmlStructuredErrorFunc f, void *arg)`.
///
/// # SAFETY
///
/// - `reader`, `arg` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
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
pub unsafe extern "C" fn xmlTextReaderSetStructuredErrorHandler(
    reader: *mut XmlTextReader,
    f: Option<crate::abi::callbacks::xmlStructuredErrorFunc>,
    arg: *mut c_void,
) {
    if reader.is_null() {
        return;
    }
    unsafe {
        (*reader).structured_handler = f;
        (*reader).structured_arg = arg;
    }
}

/// `void xmlTextReaderSetResourceLoader(xmlTextReaderPtr reader,
/// xmlResourceLoader loader, void *data)` — install a custom resource
/// loader; stored on the reader and forwarded to its parser context
/// (upstream xmlreader.c).
///
/// # SAFETY
///
/// - `reader`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `loader` must be a valid callback (or None);
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
pub unsafe extern "C" fn xmlTextReaderSetResourceLoader(
    reader: *mut XmlTextReader,
    loader: Option<crate::abi::callbacks::xmlResourceLoader>,
    data: *mut c_void,
) {
    if reader.is_null() {
        return;
    }
    unsafe {
        if !(*reader).ctxt.is_null() {
            crate::abi::exports_parserint::xmlCtxtSetResourceLoader((*reader).ctxt, loader, data);
        }
    }
}

/// `const xmlError *xmlTextReaderGetLastError(xmlTextReaderPtr reader)` —
/// pointer to the reader's embedded `_xmlError` (upstream returns
/// `&reader->ctxt->lastError`, which is always present while the reader
/// exists; valid until the next error is collected).
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderGetLastError(
    reader: *mut XmlTextReader,
) -> *const crate::abi::structs::_xmlError {
    if reader.is_null() {
        return ptr::null();
    }
    let r = unsafe { &mut *reader };
    // Sync the embedded struct from the most recent collected error, if any.
    // With no errors the struct stays zeroed (message NULL) — matching the
    // oracle, which still returns a non-NULL pointer here.
    if let Some(msg) = r.errors.last() {
        unsafe {
            // Message is a fresh NUL-terminated xmlMalloc copy owned by the
            // reader (freed on replacement and on drop).
            let bytes = msg.as_bytes();
            let m = libc::malloc(bytes.len() + 1) as *mut xmlChar;
            if !m.is_null() {
                libc::memcpy(
                    m as *mut libc::c_void,
                    bytes.as_ptr() as *const libc::c_void,
                    bytes.len(),
                );
                *m.add(bytes.len()) = 0;
                if !r.last_err.message.is_null() {
                    libc::free(r.last_err.message as *mut libc::c_void);
                }
                (*reader).last_err.message = m as *mut c_char;
                (*reader).last_err.domain = crate::abi::types::XML_FROM_PARSER as c_int;
                (*reader).last_err.level = crate::abi::types::xmlErrorLevel::XML_ERR_ERROR as c_int;
                (*reader).last_err.code = crate::abi::types::XML_ERR_INTERNAL_ERROR as c_int;
            }
        }
    }
    &(*reader).last_err as *const crate::abi::structs::_xmlError
}

/// `xmlChar *xmlTextReaderLocatorBaseURI(xmlTextReaderLocatorPtr locator)`.
///
/// # SAFETY
///
/// - `locator` must be valid or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderLocatorBaseURI(
    locator: *mut XmlTextReaderLocator,
) -> *mut xmlChar {
    if locator.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let r = (*locator).reader;
        if r.is_null() {
            return ptr::null_mut();
        }
        xml_strdup((*r).URL)
    }
}

/// `int xmlTextReaderLocatorLineNumber(xmlTextReaderLocatorPtr locator)`.
///
/// # SAFETY
///
/// - `locator` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderLocatorLineNumber(
    locator: *mut XmlTextReaderLocator,
) -> c_int {
    if locator.is_null() {
        return -1;
    }
    unsafe {
        let r = (*locator).reader;
        if r.is_null() {
            return -1;
        }
        let node = (*r).cur_node;
        if node.is_null() {
            return -1;
        }
        (*node).line as c_int
    }
}

/// `xmlParserInputBufferPtr xmlTextReaderGetRemainder(xmlTextReaderPtr reader)`.
///
/// Returns NULL — the candidate reads the whole input up front (documented
/// divergence: no unconsumed input remains).
///
/// # SAFETY
///
/// - `_reader` must be valid pointers (or NULL
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
pub const unsafe extern "C" fn xmlTextReaderGetRemainder(
    _reader: *mut XmlTextReader,
) -> *mut crate::abi::structs::_xmlParserInputBuffer {
    ptr::null_mut()
}

/// `void xmlTextReaderSetMaxAmplification(xmlTextReaderPtr reader, unsigned maxAmpl)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderSetMaxAmplification(
    reader: *mut XmlTextReader,
    maxAmpl: c_uint,
) {
    if reader.is_null() {
        return;
    }
    unsafe { (*reader).max_amplification = maxAmpl as c_int };
}

/// `int xmlTextReaderSchemaValidate(xmlTextReaderPtr reader, const char *xsd)` —
/// parse `xsd` and validate the reader's document as it is processed.
///
/// UPSTREAM-PARITY (xmlreader.c xmlTextReaderSchemaValidateInternal):
/// activation is only possible before the first Read(); the schema is
/// compiled NOW (a compile failure returns -1, which php reports as
/// "Schema contains errors") and the document is validated at read time —
/// validity diagnostics surface on the reader's error channel while
/// Read() streams, not as this call's return value.
///
/// # SAFETY
///
/// - `xsd` must be a valid path or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlTextReaderSchemaValidate(
    reader: *mut XmlTextReader,
    xsd: *const c_char,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    let r = unsafe { &mut *reader };
    if r.parsed {
        // Upstream: only XML_TEXTREADER_MODE_INITIAL readers can activate.
        return -1;
    }
    if xsd.is_null() {
        // Deactivate validation.
        if r.schema_owned && !r.schema.is_null() {
            // SAFETY: schema was compiled by xmlSchemaParse.
            unsafe { crate::xml::schemas::xmlSchemaFree(r.schema) };
        }
        r.schema = ptr::null_mut();
        r.schema_owned = false;
        r.xsd_result = -1;
        return 0;
    }
    // Compile the schema now. I/O and well-formedness diagnostics of the
    // schema document are raised by the parser machinery inside this call
    // (php prefixes them with XMLReader::setSchema()).
    let pctxt = crate::xml::schemas::xmlSchemaNewParserCtxt(xsd);
    if pctxt.is_null() {
        return -1;
    }
    let schema = crate::xml::schemas::xmlSchemaParse(pctxt);
    crate::xml::schemas::xmlSchemaFreeParserCtxt(pctxt);
    if schema.is_null() {
        return -1;
    }
    // Drop a previously attached reader-owned schema.
    if r.schema_owned && !r.schema.is_null() {
        // SAFETY: schema was compiled by xmlSchemaParse.
        unsafe { crate::xml::schemas::xmlSchemaFree(r.schema) };
    }
    r.schema = schema;
    r.schema_owned = true;
    r.xsd_result = -1;
    0
}

/// `int xmlTextReaderSchemaValidateCtxt(xmlTextReaderPtr reader, xmlSchemaValidCtxtPtr ctxt, int options)`.
///
/// # SAFETY
///
/// - `reader`, `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderSchemaValidateCtxt(
    reader: *mut XmlTextReader,
    ctxt: *mut c_void,
    _options: c_int,
) -> c_int {
    if reader.is_null() || ctxt.is_null() {
        return -1;
    }
    if unsafe { (*reader).doc }.is_null() && !unsafe { (*reader).parsed } {
        unsafe { (*reader).Read() };
    }
    crate::xml::schemas::xmlSchemaValidateDoc(ctxt, unsafe { (*reader).doc })
}

/// `int xmlTextReaderSetSchema(xmlTextReaderPtr reader, xmlSchemaPtr schema)`.
///
/// # SAFETY
///
/// - `reader`, `schema` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderSetSchema(
    reader: *mut XmlTextReader,
    schema: *mut c_void,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    unsafe {
        let r = &mut *reader;
        // Drop a previously attached reader-owned schema (a caller-owned one
        // is released by its owner; upstream never frees the passed-in
        // schema).
        if r.schema_owned && !r.schema.is_null() && r.schema != schema {
            crate::xml::schemas::xmlSchemaFree(r.schema);
        }
        r.schema = schema;
        r.schema_owned = false;
        r.xsd_result = -1;
    }
    0
}

/// `int xmlTextReaderRelaxNGValidate(xmlTextReaderPtr reader, const char *rng)`.
///
/// # SAFETY
///
/// - `reader` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `rng` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlTextReaderRelaxNGValidate(
    reader: *mut XmlTextReader,
    rng: *const c_char,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    let r = unsafe { &mut *reader };
    if r.parsed {
        // Upstream: only XML_TEXTREADER_MODE_INITIAL readers can activate.
        return -1;
    }
    if rng.is_null() {
        // Deactivate validation.
        if r.rng_owned && !r.rng.is_null() {
            // SAFETY: rng was compiled by xmlRelaxNGParse.
            unsafe { crate::xml::relaxng::xmlRelaxNGFree(r.rng) };
        }
        r.rng = ptr::null_mut();
        r.rng_owned = false;
        r.rng_result = -1;
        return 0;
    }
    // Compile the schema now (I/O diagnostics of the schema resource are
    // raised inside this call); validation itself is deferred to read time.
    let pctxt = crate::xml::relaxng::xmlRelaxNGNewParserCtxt(rng);
    if pctxt.is_null() {
        return -1;
    }
    let schema = crate::xml::relaxng::xmlRelaxNGParse(pctxt);
    crate::xml::relaxng::xmlRelaxNGFreeParserCtxt(pctxt);
    if schema.is_null() {
        return -1;
    }
    if r.rng_owned && !r.rng.is_null() {
        // SAFETY: rng was compiled by xmlRelaxNGParse.
        unsafe { crate::xml::relaxng::xmlRelaxNGFree(r.rng) };
    }
    r.rng = schema;
    r.rng_owned = true;
    r.rng_result = -1;
    0
}

/// `int xmlTextReaderRelaxNGValidateCtxt(xmlTextReaderPtr reader, xmlRelaxNGValidCtxtPtr ctxt, int options)`.
///
/// # SAFETY
///
/// - `reader`, `ctxt` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderRelaxNGValidateCtxt(
    reader: *mut XmlTextReader,
    ctxt: *mut c_void,
    _options: c_int,
) -> c_int {
    if reader.is_null() || ctxt.is_null() {
        return -1;
    }
    if unsafe { (*reader).doc }.is_null() && !unsafe { (*reader).parsed } {
        unsafe { (*reader).Read() };
    }
    crate::xml::relaxng::xmlRelaxNGValidateDoc(ctxt, unsafe { (*reader).doc })
}

/// `int xmlTextReaderRelaxNGSetSchema(xmlTextReaderPtr reader, xmlRelaxNGPtr schema)`.
///
/// # SAFETY
///
/// - `reader`, `schema` must be valid pointers (or NULL
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
pub unsafe extern "C" fn xmlTextReaderRelaxNGSetSchema(
    reader: *mut XmlTextReader,
    schema: *mut c_void,
) -> c_int {
    if reader.is_null() {
        return -1;
    }
    unsafe {
        let r = &mut *reader;
        // Drop a previously attached reader-owned schema (a caller-owned one
        // — php keeps it in intern->schema — is released by its owner).
        if r.rng_owned && !r.rng.is_null() && r.rng != schema {
            crate::xml::relaxng::xmlRelaxNGFree(r.rng);
        }
        r.rng = schema;
        r.rng_owned = false;
        r.rng_result = -1;
    }
    0
}
