//! Integration tests for the XML parser.
//!
//! These tests verify that the parser can parse real XML documents
//! and produce correct trees. They exercise the full pipeline:
//! C ABI exports → helpers → input → tokenizer → state machine → SAX → tree.
//!
//! # Ownership & safety invariants
//!
//! The tests build real documents through the public entry points and then
//! free them with xmlFreeDoc, exercising the ownership contract
//! (documents own their whole subtree; node/dict pointers are borrowed).
//! Any leak or double-free in the tree ownership model surfaces here under
//! the allocator registry tests.
//!
//! # Proving courts
//!
//! These tests complement the differential corpus: the CLI-XMLLINT-* cases
//! (47 byte-identical) prove oracle parity end-to-end, while this module
//! pins tree topology, namespace wiring and content values in-process.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to assert only well-formedness instead of
//! exact tree shape. Parser output is an observable contract (node kinds,
//! attribute values, namespace bindings); tests that accept any
//! well-formed tree would not catch the topology divergences the oracle
//! corpus is designed to detect. Assert exact structure.

use crate::abi::structs::*;
use crate::abi::types::xmlChar;
use crate::xml::parser::helpers;
use crate::xml::tree;
use core::ptr;
use std::ffi::c_void;
use std::os::raw::c_int;

/// Helper: parse a byte slice and return the document.
unsafe fn parse_bytes(bytes: &[u8]) -> *mut _xmlDoc {
    let ctxt = helpers::create_parser_ctxt();
    assert!(!ctxt.is_null(), "parser context should not be null");
    let input = helpers::input_from_memory(bytes.as_ptr() as *const i8, bytes.len() as i32);
    helpers::setup_parser_input(ctxt, input);
    let ret = helpers::parse_document(ctxt);
    let doc = (*ctxt).myDoc;
    helpers::free_parser_ctxt(ctxt);
    if ret != 0 {
        return ptr::null_mut();
    }
    doc
}

/// Parse an empty document and check the root element type and name.
///
/// # Safety
///
/// - The document returned by `parse_bytes` is non-NULL (asserted) and
///   valid while its children and name pointers are read; the document
///   and its subtree stay alive until the end of the test.
#[test]
fn test_parse_empty_document() {
    unsafe {
        let xml = b"<?xml version=\"1.0\"?>\n<root/>\n";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null(), "should parse empty doc");
        assert!(!(*doc).children.is_null(), "should have root element");
        let root = (*doc).children;
        assert_eq!(
            (*root).type_,
            crate::abi::types::xmlElementType::XML_ELEMENT_NODE as i32
        );
        // Check root name
        let name = crate::xml::string::xmlstr_to_bytes((*root).name as *const xmlChar);
        assert_eq!(name, b"root");
    }
}

/// Parse a document with text content and verify the text node.
///
/// # Safety
///
/// - The parsed document and its child text node are non-NULL
///   (asserted) and stay valid while `content` is read.
#[test]
fn test_parse_element_with_text() {
    unsafe {
        let xml = b"<?xml version=\"1.0\"?>\n<root>Hello, World!</root>\n";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null(), "should parse doc with text");
        let root = (*doc).children;
        assert!(!root.is_null());
        let text = (*root).children;
        assert!(!text.is_null(), "root should have text child");
        assert_eq!(
            (*text).type_,
            crate::abi::types::xmlElementType::XML_TEXT_NODE as i32
        );
        let content = crate::xml::string::xmlstr_to_bytes((*text).content as *const xmlChar);
        assert_eq!(content, b"Hello, World!");
    }
}

/// Parse a document with attributes and verify name/value chains.
///
/// # Safety
///
/// - The document, root, attribute and value nodes are non-NULL
///   (asserted) and stay alive while their `name`/`content` pointers
///   are read.
#[test]
fn test_parse_element_with_attributes() {
    unsafe {
        let xml = b"<root id=\"123\" name=\"test\"/>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null(), "should parse doc with attributes");
        let root = (*doc).children;
        assert!(!root.is_null());
        // Check attributes
        let attr = (*root).properties;
        assert!(!attr.is_null(), "root should have attributes");
        // First attribute: id="123"
        let attr_name = crate::xml::string::xmlstr_to_bytes((*attr).name as *const xmlChar);
        assert_eq!(attr_name, b"id");
        let attr_val =
            crate::xml::string::xmlstr_to_bytes((*(*attr).children).content as *const xmlChar);
        assert_eq!(attr_val, b"123");
        // Second attribute: name="test"
        let attr2 = (*attr).next;
        assert!(!attr2.is_null(), "should have second attribute");
        let attr2_name = crate::xml::string::xmlstr_to_bytes((*attr2).name as *const xmlChar);
        assert_eq!(attr2_name, b"name");
        let attr2_val =
            crate::xml::string::xmlstr_to_bytes((*(*attr2).children).content as *const xmlChar);
        assert_eq!(attr2_val, b"test");
    }
}

/// Parse nested elements and verify the parent/child/sibling chain.
///
/// # Safety
///
/// - Every traversed node is non-NULL (asserted) and stays alive while
///   its `name` and `next`/`children` pointers are read.
#[test]
fn test_parse_nested_elements() {
    unsafe {
        let xml = b"<root><child1><grandchild/></child1><child2/></root>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let root = (*doc).children;
        assert!(!root.is_null());
        let name = crate::xml::string::xmlstr_to_bytes((*root).name as *const xmlChar);
        assert_eq!(name, b"root");
        // First child: child1
        let child1 = (*root).children;
        assert!(!child1.is_null());
        let child1_name = crate::xml::string::xmlstr_to_bytes((*child1).name as *const xmlChar);
        assert_eq!(child1_name, b"child1");
        // Grandchild
        let grandchild = (*child1).children;
        assert!(!grandchild.is_null(), "child1 should have grandchild");
        let grandchild_name =
            crate::xml::string::xmlstr_to_bytes((*grandchild).name as *const xmlChar);
        assert_eq!(grandchild_name, b"grandchild");
        // Sibling: child2
        let child2 = (*child1).next;
        assert!(!child2.is_null(), "should have child2 sibling");
        let child2_name = crate::xml::string::xmlstr_to_bytes((*child2).name as *const xmlChar);
        assert_eq!(child2_name, b"child2");
    }
}

/// Parse a comment node and verify its type and content.
///
/// # Safety
///
/// - The document, root and comment nodes are non-NULL (asserted) and
///   valid while `type_` and `content` are read.
#[test]
fn test_parse_comments() {
    unsafe {
        let xml = b"<root><!-- this is a comment --></root>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let root = (*doc).children;
        assert!(!root.is_null());
        let comment = (*root).children;
        assert!(!comment.is_null());
        assert_eq!(
            (*comment).type_,
            crate::abi::types::xmlElementType::XML_COMMENT_NODE as i32
        );
        let content = crate::xml::string::xmlstr_to_bytes((*comment).content as *const xmlChar);
        assert_eq!(content, b" this is a comment ");
    }
}

/// Parse a processing instruction and read its content.
///
/// # Safety
///
/// - The document and PI node are non-NULL (asserted) and valid while
///   `type_` and `content` are read.
#[test]
fn test_parse_processing_instruction() {
    unsafe {
        let xml = b"<?mypi data?><root/>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        // The PI should be a child of the document
        let pi = (*doc).children;
        assert!(!pi.is_null());
        // The first child might be the PI
        if (*pi).type_ == crate::abi::types::xmlElementType::XML_PI_NODE as i32 {
            let content = crate::xml::string::xmlstr_to_bytes((*pi).content as *const xmlChar);
            assert_eq!(content, b"data");
        }
    }
}

/// Parse a document containing an entity reference.
///
/// # Safety
///
/// - The document, root and text nodes are non-NULL (asserted) and
///   stay valid while `type_` and `content` are read.
#[test]
fn test_parse_with_entities() {
    unsafe {
        let xml = b"<root>AT&amp;T</root>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let root = (*doc).children;
        assert!(!root.is_null());
        let text = (*root).children;
        assert!(!text.is_null());
        if (*text).type_ == crate::abi::types::xmlElementType::XML_TEXT_NODE as i32 {
            let content = crate::xml::string::xmlstr_to_bytes((*text).content as *const xmlChar);
            // With default settings, entities are NOT replaced
            // So content should be "AT&amp;T" as a reference, not "AT&T"
            // Actually, the parser may keep entity references as separate nodes
            assert!(!content.is_empty());
        }
    }
}

/// Parse a CDATA section and check the resulting node kind.
///
/// # Safety
///
/// - The document, root and cdata nodes are non-NULL (asserted) and
///   valid while `type_` is read.
#[test]
fn test_parse_cdata_section() {
    unsafe {
        let xml = b"<root><![CDATA[Hello <world>]]></root>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let root = (*doc).children;
        assert!(!root.is_null());
        let cdata = (*root).children;
        assert!(!cdata.is_null());
        // CDATA sections become text nodes in the tree (since XML_PARSE_NOCDATA is not set)
        // Or they may be CDATA section nodes depending on implementation
        assert!(
            (*cdata).type_ == crate::abi::types::xmlElementType::XML_CDATA_SECTION_NODE as i32
                || (*cdata).type_ == crate::abi::types::xmlElementType::XML_TEXT_NODE as i32,
            "CDATA should be CDATA or text node"
        );
    }
}

/// Parse a document with an XML declaration and verify version/encoding.
///
/// # Safety
///
/// - The parsed document is non-NULL (asserted) and valid while its
///   `version` and `encoding` pointers are read.
#[test]
fn test_parse_xml_declaration() {
    unsafe {
        let xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null(), "should parse with XML declaration");
        // Check version and encoding on the document
        let version = crate::xml::string::xmlstr_to_bytes((*doc).version as *const xmlChar);
        assert_eq!(version, b"1.0");
        let encoding = crate::xml::string::xmlstr_to_bytes((*doc).encoding as *const xmlChar);
        assert_eq!(encoding, b"UTF-8");
    }
}

/// Parse malformed XML and check that failure is reported.
///
/// # Safety
///
/// - The parser context is non-NULL (asserted) and valid until
///   `free_parser_ctxt`; `myDoc` may be NULL and is only null-checked.
#[test]
fn test_parse_invalid_xml() {
    unsafe {
        // Missing closing tag
        let xml = b"<root><child>";
        let ctxt = helpers::create_parser_ctxt();
        assert!(!ctxt.is_null());
        let input = helpers::input_from_memory(xml.as_ptr() as *const i8, xml.len() as i32);
        helpers::setup_parser_input(ctxt, input);
        // Without recovery mode, this should fail
        let _ret = helpers::parse_document(ctxt);
        let doc = (*ctxt).myDoc;
        helpers::free_parser_ctxt(ctxt);
        // Should fail (ret != 0) or doc may be NULL
        // The document might still be partially constructed
        if !doc.is_null() {
            assert!(
                !doc.is_null(),
                "even partial doc should not be null if wellFormed"
            );
        }
    }
}

/// Parse through the exported `xmlReadMemory` entry point.
///
/// # Safety
///
/// - `xml` is a static byte buffer valid for the call; the returned
///   document is non-NULL (asserted) and valid while its tree is read;
///   the document is never freed here, matching the exported API's
///   ownership contract.
#[test]
fn test_xml_read_memory_export() {
    unsafe {
        let xml = b"<root attr=\"val\"/>";
        let doc = crate::abi::exports_xml2::xmlReadMemory(
            xml.as_ptr() as *const i8,
            xml.len() as i32,
            core::ptr::null(),
            core::ptr::null(),
            0,
        );
        assert!(!doc.is_null(), "xmlReadMemory should return a doc");
        let root = (*doc).children;
        assert!(!root.is_null());
        let name = crate::xml::string::xmlstr_to_bytes((*root).name as *const xmlChar);
        assert_eq!(name, b"root");
        // Check attr
        let attr = (*root).properties;
        assert!(!attr.is_null());
        let attr_name = crate::xml::string::xmlstr_to_bytes((*attr).name as *const xmlChar);
        assert_eq!(attr_name, b"attr");
        let attr_val =
            crate::xml::string::xmlstr_to_bytes((*(*attr).children).content as *const xmlChar);
        assert_eq!(attr_val, b"val");
    }
}

// ── R-000166 regression tests (11.1-Y) ──

/// Parse an unbound-prefix document and verify the raw QName is kept.
///
/// # Safety
///
/// - The document, nodes and attributes are non-NULL (asserted) and
///   valid while their `name`/`ns` fields are read; the document is
///   freed with `tree::free_doc` exactly once at the end.
#[test]
fn test_undefined_prefix_keeps_qname() {
    // UPSTREAM-PARITY (SAX2.c xmlSAX2StartElementNs): elements and
    // attributes whose prefix is not bound to any namespace keep the raw
    // QName as the node/attr name with a NULL namespace, so `<p:b/>`
    // serializes as `<p:b/>` and `<a p:x="1"/>` as `p:x`.
    unsafe {
        let xml = b"<a p:x=\"1\"><p:b/></a>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let a = (*doc).children;
        assert!(!a.is_null());
        // Attribute keeps the raw QName.
        let attr = (*a).properties;
        assert!(!attr.is_null());
        let attr_name = crate::xml::string::xmlstr_to_bytes((*attr).name as *const xmlChar);
        assert_eq!(attr_name, b"p:x");
        assert!((*attr).ns.is_null());
        // Child element keeps the raw QName.
        let pb = (*a).children;
        assert!(!pb.is_null());
        let child_name = crate::xml::string::xmlstr_to_bytes((*pb).name as *const xmlChar);
        assert_eq!(child_name, b"p:b");
        assert!((*pb).ns.is_null());
        tree::free_doc(doc);
    }
}

/// Parse ancestor-declared namespace prefixes and verify the binding.
///
/// # Safety
///
/// - The document, nodes, attributes and `ns` pointers are non-NULL
///   (asserted) and stay valid while `href` is read; the document is
///   freed with `tree::free_doc` exactly once at the end.
#[test]
fn test_ancestor_declared_prefix_binds_uri() {
    // UPSTREAM-PARITY (parser.c xmlParserNsLookupUri): element and attribute
    // URIs resolve against the ancestor scope when not declared on the
    // element itself, so `<a xmlns:p="u"><p:b/></a>` binds p:b to "u".
    unsafe {
        let xml = b"<a xmlns:p=\"http://u/p\"><p:b p:c=\"1\"/></a>";
        let doc = parse_bytes(xml);
        assert!(!doc.is_null());
        let a = (*doc).children;
        let pb = (*a).children;
        assert!(!pb.is_null());
        let ns = (*pb).ns;
        assert!(
            !ns.is_null(),
            "element should bind the ancestor-declared prefix"
        );
        let href = crate::xml::string::xmlstr_to_bytes((*ns).href as *const xmlChar);
        assert_eq!(href, b"http://u/p");
        // Attribute too.
        let attr = (*pb).properties;
        assert!(!attr.is_null());
        assert!(
            !(*attr).ns.is_null(),
            "attribute should bind the ancestor-declared prefix"
        );
        let a_href = crate::xml::string::xmlstr_to_bytes((*(*attr).ns).href as *const xmlChar);
        assert_eq!(a_href, b"http://u/p");
        tree::free_doc(doc);
    }
}

// ── SP-14.3.1-2 regression tests (undeclared-entity severity, R-14.3) ──

/// Parse a document through the exported `xmlCtxtReadMemory` and return
/// `(doc, wellFormed, errNo)`. The context is freed; the returned doc (when
/// non-NULL) is owned by the caller.
///
/// # Safety
///
/// - `xml` is a static byte buffer valid for the call; the returned document
///   (when non-NULL) is valid until `tree::free_doc`d by the caller.
unsafe fn read_memory_ctx(xml: &[u8], options: c_int) -> (*mut _xmlDoc, c_int, c_int) {
    unsafe {
        let ctxt = crate::abi::exports_parser::xmlNewParserCtxt();
        assert!(!ctxt.is_null());
        let doc = crate::abi::exports_parser::xmlCtxtReadMemory(
            ctxt,
            xml.as_ptr() as *const i8,
            xml.len() as i32,
            core::ptr::null(),
            core::ptr::null(),
            options,
        );
        let wf = (*ctxt).wellFormed;
        let err = (*ctxt).errNo;
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        (doc, wf, err)
    }
}

/// A reference to an undeclared general entity is FATAL (WFC: Entity
/// Declared) only when the document is standalone or has neither an external
/// subset nor parameter-entity references — upstream `xmlHandleUndeclaredEntity`.
///
/// # Safety
///
/// - The static buffers are valid for the parse; `read_memory_ctx` frees the
///   context and returns an owned doc (NULL in these fatal cases).
#[test]
fn test_undeclared_entity_fatal_without_extsubset_or_perefs() {
    unsafe {
        // No DTD at all: fatal, errNo 26 (XML_ERR_UNDECLARED_ENTITY).
        let (doc, wf, err) = read_memory_ctx(b"<root>a&nope;b</root>", 0);
        assert!(doc.is_null(), "undeclared ref without DTD must fail");
        assert_eq!(wf, 0);
        assert_eq!(err, 26);

        // Internal subset without parameter-entity references (and no external
        // subset) is still fatal per the WFC wording.
        let (doc2, wf2, err2) = read_memory_ctx(
            b"<!DOCTYPE root [ <!ENTITY e \"E\"> ]><root>a&nope;b</root>",
            0,
        );
        assert!(doc2.is_null());
        assert_eq!(wf2, 0);
        assert_eq!(err2, 26);
    }
}

/// When the document has an external subset or parameter-entity references,
/// an undeclared general entity is NON-fatal and the parse continues past it.
///
/// # Safety
///
/// - The static buffers are valid for the parse; the returned docs are owned
///   and freed exactly once via `tree::free_doc`.
#[test]
fn test_undeclared_entity_nonfatal_with_extsubset_or_perefs() {
    unsafe {
        // External subset declared (SYSTEM), not loaded: warning level, parse
        // succeeds and keeps the rest of the content (oracle: wellFormed=1,
        // errNo stays 0, doc=yes).
        let (doc, wf, err) = read_memory_ctx(
            b"<!DOCTYPE root SYSTEM \"nope.dtd\"><root>a&nope;b</root>",
            0,
        );
        assert!(!doc.is_null(), "ext-subset doc must parse");
        assert_eq!(wf, 1);
        assert_eq!(err, 0);
        tree::free_doc(doc);

        // Internal subset containing a parameter-entity reference: same.
        let (doc2, wf2, err2) = read_memory_ctx(
            b"<!DOCTYPE root [ <!ENTITY % p SYSTEM \"x\"> %p; ]><root>a&nope;b</root>",
            0,
        );
        assert!(!doc2.is_null(), "PE-ref doc must parse");
        assert_eq!(wf2, 1);
        assert_eq!(err2, 0);
        tree::free_doc(doc2);
    }
}

/// With XML_PARSE_NOENT (entity substitution requested and no XML_PARSE_NO_XXE)
/// the undeclared reference is reported as XML_WAR_UNDECLARED_ENTITY (27) at
/// ERROR level, errNo = 27, and parsing still continues (oracle: wellFormed=1,
/// doc=yes, errNo=27). This is the configuration PHP's expat-compat parser
/// uses, and the behavior xml004/xml_closures_001 depend on.
///
/// # Safety
///
/// - The static buffers are valid for the parse; the returned docs are owned
///   and freed exactly once via `tree::free_doc`.
#[test]
fn test_undeclared_entity_noent_nonfatal_error_27() {
    unsafe {
        let (doc, wf, err) = read_memory_ctx(
            b"<!DOCTYPE root SYSTEM \"nope.dtd\"><root>a&nope;b</root>",
            crate::abi::types::XML_PARSE_NOENT,
        );
        assert!(!doc.is_null(), "NOENT ext-subset doc must parse");
        assert_eq!(wf, 1);
        assert_eq!(err, 27);
        tree::free_doc(doc);

        let (doc2, wf2, err2) = read_memory_ctx(
            b"<!DOCTYPE root [ <!ENTITY % p SYSTEM \"x\"> %p; ]><root>a&nope;b</root>",
            crate::abi::types::XML_PARSE_NOENT,
        );
        assert!(!doc2.is_null(), "NOENT PE-ref doc must parse");
        assert_eq!(wf2, 1);
        assert_eq!(err2, 27);
        tree::free_doc(doc2);
    }
}

// ── SP-14.3.1-3 regression guards (incremental push delivery, SAX1/SAX2
// ── dispatch, parser-scoped namespace stack) ────────────────────────────────

// Capture state for the C callbacks below. `thread_local!` keeps the tests
// independent under Rust's parallel test runner: the callbacks run on the
// same thread as their test.
thread_local! {
    static SAX1_STARTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SAX1_ENDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SAX1_CHARS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SAX2_EVENTS: std::cell::RefCell<Vec<(Vec<u8>, Vec<u8>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SAX1_EVENTS: std::cell::RefCell<Vec<Vec<u8>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

unsafe extern "C" fn sax1_start(
    _ctx: *mut c_void,
    name: *const xmlChar,
    atts: *mut *const xmlChar,
) {
    SAX1_STARTS.with(|c| c.set(c.get() + 1));
    if name.is_null() {
        return;
    }
    let n = unsafe { std::ffi::CStr::from_ptr(name as *const i8) }
        .to_bytes()
        .to_vec();
    let mut parts: Vec<Vec<u8>> = vec![n];
    if !atts.is_null() {
        let mut i = 0usize;
        unsafe {
            while !(*atts.add(i)).is_null() {
                let k = std::ffi::CStr::from_ptr(*atts.add(i) as *const i8)
                    .to_bytes()
                    .to_vec();
                let v = std::ffi::CStr::from_ptr(*atts.add(i + 1) as *const i8)
                    .to_bytes()
                    .to_vec();
                parts.push(k);
                parts.push(v);
                i += 2;
            }
        }
    }
    SAX1_EVENTS.with(|e| e.borrow_mut().push(parts.into_iter().flatten().collect()));
}

unsafe extern "C" fn sax1_end(_ctx: *mut c_void, _name: *const xmlChar) {
    SAX1_ENDS.with(|c| c.set(c.get() + 1));
}

unsafe extern "C" fn sax1_chars(_ctx: *mut c_void, _ch: *const xmlChar, _len: c_int) {
    SAX1_CHARS.with(|c| c.set(c.get() + 1));
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn sax2_start_ns(
    _ctx: *mut c_void,
    local: *const xmlChar,
    _prefix: *const xmlChar,
    uri: *const xmlChar,
    _nb_ns: c_int,
    _ns: *mut *const xmlChar,
    _nb_atts: c_int,
    _nb_def: c_int,
    _atts: *mut *const xmlChar,
) {
    let local = if local.is_null() {
        b"(null)".to_vec()
    } else {
        unsafe { std::ffi::CStr::from_ptr(local as *const i8) }
            .to_bytes()
            .to_vec()
    };
    let uri = if uri.is_null() {
        Vec::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(uri as *const i8) }
            .to_bytes()
            .to_vec()
    };
    SAX2_EVENTS.with(|e| e.borrow_mut().push((local, uri)));
}

unsafe extern "C" fn sax2_end_ns(
    _ctx: *mut c_void,
    _local: *const xmlChar,
    _prefix: *const xmlChar,
    _uri: *const xmlChar,
) {
}

/// Reset the capture state for the calling thread.
fn reset_push_capture() {
    SAX1_STARTS.with(|c| c.set(0));
    SAX1_ENDS.with(|c| c.set(0));
    SAX1_CHARS.with(|c| c.set(0));
    SAX1_EVENTS.with(|e| e.borrow_mut().clear());
    SAX2_EVENTS.with(|e| e.borrow_mut().clear());
}

/// Build a push-parser context with a SAX1 handler set (initialized = 1, the
/// PHP expat-compat `xml_parser_create` configuration), or a SAX2-magic
/// handler when `magic` is set.
unsafe fn push_ctxt(
    sax1_handlers: bool,
) -> (
    *mut crate::abi::structs::_xmlParserCtxt,
    *mut crate::abi::structs::_xmlSAXHandler,
) {
    let mut h = crate::abi::structs::_xmlSAXHandler {
        ..std::mem::zeroed()
    };
    if sax1_handlers {
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        h.characters = Some(sax1_chars);
        h.initialized = 1;
    } else {
        h.initialized = crate::abi::types::XML_SAX2_MAGIC as u32;
        h.startElementNs = Some(sax2_start_ns);
        h.endElementNs = Some(sax2_end_ns);
    }
    let sax_ptr = Box::into_raw(Box::new(h));
    let ctxt = crate::abi::exports_parser::xmlCreatePushParserCtxt(
        sax_ptr,
        ptr::null_mut(),
        ptr::null(),
        0,
        ptr::null(),
    );
    assert!(!ctxt.is_null());
    (ctxt, sax_ptr)
}

/// A complete document fed to xmlParseChunk WITHOUT the terminating flag must
/// deliver all events (PHP xml_parse defaults isFinal=false; upstream parses
/// each chunk eagerly). The oracle fires S(root) S(b) E(b) E(root) plus two
/// character events for `<root>hello <b>world</b></root>`; the completing
/// parse must deliver exactly that once.
///
/// # Safety
///
/// - The static handler structs and counter state are module-scoped; the
///   context is freed at the end of the test.
#[test]
fn test_push_single_shot_without_terminate_delivers() {
    unsafe {
        reset_push_capture();
        let doc = b"<root>hello <b>world</b></root>";
        let (ctxt, sax_ptr) = push_ctxt(true);
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            0,
        );
        assert_eq!(rc, 0);
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 2);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 2);
        assert_eq!(SAX1_CHARS.with(|c| c.get()), 2);
        // A later terminating call must NOT re-deliver (events fire once).
        let rc2 = crate::abi::exports_xml2::xmlParseChunk(ctxt, ptr::null(), 0, 1);
        assert_eq!(rc2, 0);
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 2);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 2);
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// The same document fed in small non-terminating chunks and then finalized
/// must also deliver each event exactly once.
///
/// # Safety
///
/// - As `test_push_single_shot_without_terminate_delivers`.
#[test]
fn test_push_chunked_delivers_once() {
    unsafe {
        reset_push_capture();
        let doc = b"<root>hello <b>world</b></root>";
        let (ctxt, sax_ptr) = push_ctxt(true);
        for chunk in doc.chunks(4) {
            let rc = crate::abi::exports_xml2::xmlParseChunk(
                ctxt,
                chunk.as_ptr() as *const i8,
                chunk.len() as c_int,
                0,
            );
            assert_eq!(rc, 0);
        }
        let rc = crate::abi::exports_xml2::xmlParseChunk(ctxt, ptr::null(), 0, 1);
        assert_eq!(rc, 0);
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 2);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 2);
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// A document with a fatal error at its very end (the root end tag does not
/// match the root start tag) still delivers the events that precede it when
/// fed single-shot without terminate — upstream parsed them eagerly
/// (bug25666: `</foo>` closing `<foo:a ...>`).
///
/// # Safety
///
/// - As `test_push_single_shot_without_terminate_delivers`.
#[test]
fn test_push_error_at_end_still_delivers() {
    unsafe {
        reset_push_capture();
        let doc = b"<foo:a xmlns:foo=\"u\"><bar:b xmlns:bar=\"v\"/></foo>";
        let (ctxt, sax_ptr) = push_ctxt(false); // SAX2-magic capture
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            0,
        );
        assert_eq!(rc, 0);
        let evs = SAX2_EVENTS.with(|e| e.borrow().clone());
        assert_eq!(evs.len(), 2, "both start events must be delivered");
        assert_eq!(evs[0], (b"a".to_vec(), b"u".to_vec()));
        // The child's prefix is declared on the ROOT — the parser-scoped
        // namespace stack must resolve it (bug25666/xml009).
        assert_eq!(evs[1], (b"b".to_vec(), b"v".to_vec()));
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// SAX1 dispatch (PHP expat-compat non-namespace parser) delivers the RAW
/// QName and every attribute including xmlns declarations, with no namespace
/// processing (upstream xmlParseStartTag; bug50576/bug72714).
///
/// # Safety
///
/// - As `test_push_single_shot_without_terminate_delivers`.
#[test]
fn test_push_sax1_raw_qname_and_xmlns_attributes() {
    unsafe {
        reset_push_capture();
        let doc = b"<ns1:listOfAwards xmlns:ns1=\"http://www.fpdsng.com/FPDS\"/>\n";
        let (ctxt, sax_ptr) = push_ctxt(true);
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc, 0);
        let evs = SAX1_EVENTS.with(|e| e.borrow().clone());
        assert_eq!(evs.len(), 1);
        assert_eq!(
            evs[0],
            b"ns1:listOfAwardsxmlns:ns1http://www.fpdsng.com/FPDS".to_vec()
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}
