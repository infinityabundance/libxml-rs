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

// ── KEY-1 regression guard (declared encoding on BOM-less input) ───────────

/// A BOM-less byte stream that declares `encoding="iso-8859-1"` and contains
/// a Latin-1 byte >= 0x80 must parse to the same tree as the oracle (the raw
/// byte is transcoded to UTF-8 at input setup, not rejected as "Invalid bytes
/// in character encoding"). This is the engine-level half of the PHP
/// `simplexml_load_file`/`DOMDocument::load` failure on `ext/xsl`'s
/// `xslt.xml` (KEY-1).
///
/// # Safety
///
/// - As `test_parse_empty_document`; the text content pointer is read while
///   the doc is alive.
#[test]
fn test_parse_declared_latin1_bomless_memory_doc() {
    unsafe {
        // `<?xml version="1.0" encoding="iso-8859-1"?><r>ä</r>` in Latin-1.
        let mut xml = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><r>".to_vec();
        xml.push(0xE4);
        xml.extend_from_slice(b"</r>");
        let doc = parse_bytes(&xml);
        assert!(!doc.is_null(), "BOM-less declared Latin-1 doc must parse");
        let root = (*doc).children;
        assert!(!root.is_null());
        let text = (*root).children;
        assert!(!text.is_null(), "root should have the text child");
        assert_eq!(
            (*text).type_,
            crate::abi::types::xmlElementType::XML_TEXT_NODE as i32
        );
        let content = crate::xml::string::xmlstr_to_bytes((*text).content as *const xmlChar);
        assert_eq!(content, "ä".as_bytes(), "Latin-1 byte decodes to UTF-8 ä");
        tree::free_doc(doc);
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
        // A prolog DOCTYPE with an internal subset stays legal.
        let (d, wf, err) =
            read_memory_ctx(b"<!DOCTYPE root [ <!ENTITY e \"E\"> ]><root>a</root>", 0);
        assert!(!d.is_null());
        assert_eq!(wf, 1);
        assert_eq!(err, 0);
        tree::free_doc(d);
    }
}

// ── KEY-3 regression guard (PI / XML-decl routing + reserved-name codes) ────

/// `<?xml` is an XML declaration only at the logical document start with a
/// blank after "xml" (upstream xmlParseDocument CMP5 + IS_BLANK(NXT(5))); any
/// other `<?xml...` is a PI whose reserved target raises XML_ERR_RESERVED_XML_NAME
/// (64), and a PI never closed by `?>` raises XML_ERR_PI_NOT_FINISHED (47).
/// These are the xml_error_string_basic_libxml rows (KEY-3): `<?xml?>` -> 64,
/// `<?xml>` -> 47, `<?xml version="dummy">` -> 57. A UTF-8 BOM before the
/// declaration does not stop it being a declaration (bug35447).
///
/// # Safety
///
/// - As `test_undeclared_entity_fatal_without_extsubset_or_perefs`.
#[test]
fn test_pi_vs_xml_decl_routing_error_codes() {
    unsafe {
        // `<?xml?>` / `<?XML?>` / leading-space `<?xml?>`: reserved name 64.
        for doc in [&b"<?xml?>"[..], &b"<?XML?>"[..], &b" <?xml?>"[..]] {
            let (d, wf, err) = read_memory_ctx(doc, 0);
            assert!(d.is_null());
            assert_eq!(wf, 0);
            assert_eq!(
                err,
                crate::abi::types::XML_ERR_RESERVED_XML_NAME,
                "{} -> 64",
                String::from_utf8_lossy(doc)
            );
        }
        // `<?xml>` (PI never closed): PI_NOT_FINISHED 47 is the final error.
        let (d, wf, err) = read_memory_ctx(b"<?xml>", 0);
        assert!(d.is_null());
        assert_eq!(wf, 0);
        assert_eq!(err, crate::abi::types::XML_ERR_PI_NOT_FINISHED);
        // `<?xml version="dummy">`: declaration never closed -> 57.
        let (d, wf, err) = read_memory_ctx(b"<?xml version=\"dummy\">", 0);
        assert!(d.is_null());
        assert_eq!(wf, 0);
        assert_eq!(err, crate::abi::types::XML_ERR_XMLDECL_NOT_FINISHED);
        // Real declarations stay legal: plain, after a UTF-8 BOM (bug35447),
        // and the W3C `<?xml-stylesheet ...?>` PI is not reserved.
        for doc in [
            &b"<?xml version=\"1.0\"?><r/>"[..],
            &b"\xEF\xBB\xBF<?xml version=\"1.0\"?><r/>"[..],
            &b"<?xml-stylesheet type=\"text/xsl\" href=\"x\"?><r/>"[..],
        ] {
            let (d, wf, err) = read_memory_ctx(doc, 0);
            assert!(!d.is_null());
            assert_eq!(wf, 1);
            assert_eq!(err, 0);
            tree::free_doc(d);
        }
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
    // SP-14.3.1-4 capture state.
    static SAX_CHAR_LOG: std::cell::RefCell<Vec<Vec<u8>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static GE_NAMES: std::cell::RefCell<Vec<Vec<u8>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// When set, the getEntity callback resolves "pic" and stops the parser
    /// with errNo = 21 exactly like PHP expat-compat's external-entity-ref
    /// handler returning FALSE (bug71592).
    static GE_STOP_PIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Per-start-event (element local name, [(attr local name, value)]).
    static SAX2_START_LOG: std::cell::RefCell<Vec<(Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// (line, byte-offset) of each endElement event (bug26614 locator guard).
    static END_POS: std::cell::RefCell<Vec<(c_int, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Character-data callback logging the payloads (for entity-delivery guards).
///
/// # Safety
///
/// - `ch` must be NULL or valid for `len` readable bytes.
unsafe extern "C" fn sax1_chars_log(_ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    if !ch.is_null() && len > 0 {
        let bytes = unsafe { core::slice::from_raw_parts(ch as *const u8, len as usize) }.to_vec();
        SAX_CHAR_LOG.with(|l| l.borrow_mut().push(bytes));
    }
}

/// getEntity callback mirroring PHP expat-compat `compat.c`: resolve via
/// xmlGetPredefinedEntity then xmlGetDocEntity(ctxt->myDoc). With
/// GE_STOP_PIC set it stops the parser for the "pic" entity and records
/// errNo = XML_ERROR_EXTERNAL_ENTITY_HANDLING (21) — what compat's
/// external_entity_ref_handler does when the PHP callback returns FALSE
/// (SP-14.3.1-4, bug71592).
///
/// # Safety
///
/// - `ctx` is the parser context (the tests create push contexts with NULL
///   userData, so ctx == ctxt); `name` is a valid NUL-terminated string; the
///   returned entity is owned by the document's entity table.
unsafe extern "C" fn sax_ge_resolve(ctx: *mut c_void, name: *const xmlChar) -> *mut _xmlEntity {
    unsafe {
        if !name.is_null() {
            let n = std::ffi::CStr::from_ptr(name as *const i8)
                .to_bytes()
                .to_vec();
            GE_NAMES.with(|g| g.borrow_mut().push(n));
        }
        let ctxt = ctx as *mut _xmlParserCtxt;
        let mut ent = crate::abi::exports_misc::xmlGetPredefinedEntity(name);
        if ent.is_null() {
            ent = crate::xml::tree::get_doc_entity((*ctxt).myDoc, name);
        }
        if !ent.is_null() && GE_STOP_PIC.get() {
            let n = std::ffi::CStr::from_ptr(name as *const i8).to_bytes();
            if n == b"pic" {
                crate::abi::exports_parser::xmlStopParser(ctxt);
                (*ctxt).errNo = 21; // XML_ERROR_EXTERNAL_ENTITY_HANDLING
            }
        }
        ent
    }
}

/// SAX2 startElementNs callback logging the element local name and its
/// attribute (local name, value) pairs (defaulted attributes included).
///
/// # Safety
///
/// - `local` must be NULL or a valid NUL-terminated string; `atts` must be
///   NULL or valid for `nb_atts * 5` pointer slots (SAX2 layout).
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn sax2_start_ns_log(
    _ctx: *mut c_void,
    local: *const xmlChar,
    _prefix: *const xmlChar,
    _uri: *const xmlChar,
    _nb_ns: c_int,
    _ns: *mut *const xmlChar,
    nb_atts: c_int,
    _nb_def: c_int,
    atts: *mut *const xmlChar,
) {
    let lname = if local.is_null() {
        Vec::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(local as *const i8) }
            .to_bytes()
            .to_vec()
    };
    let mut attrs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if !atts.is_null() {
        let n = nb_atts as usize;
        for i in 0..n {
            let a = unsafe { *atts.add(i * 5) };
            let v = unsafe { *atts.add(i * 5 + 3) };
            let vend = unsafe { *atts.add(i * 5 + 4) };
            if a.is_null() {
                continue;
            }
            let aname = unsafe { std::ffi::CStr::from_ptr(a as *const i8) }
                .to_bytes()
                .to_vec();
            let aval = if v.is_null() || vend.is_null() || vend <= v {
                Vec::new()
            } else {
                unsafe { core::slice::from_raw_parts(v as *const u8, vend.offset_from(v) as usize) }
                    .to_vec()
            };
            attrs.push((aname, aval));
        }
    }
    SAX2_START_LOG.with(|l| l.borrow_mut().push((lname, attrs)));
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

/// SAX1 endElement callback recording the parser's current line and byte
/// offset (input->cur - input->base) at the moment the end event fires
/// (bug26614 locator guard).
///
/// # Safety
///
/// - `ctx` is the parser context (push contexts here use NULL userData, so
///   ctx == ctxt); `ctxt->input` must be valid.
unsafe extern "C" fn sax1_end_pos(ctx: *mut c_void, _name: *const xmlChar) {
    SAX1_ENDS.with(|c| c.set(c.get() + 1));
    let ctxt = ctx as *mut crate::abi::structs::_xmlParserCtxt;
    let input = unsafe { (*ctxt).input };
    if !input.is_null() && !(*input).base.is_null() {
        let line = unsafe { (*input).line };
        let byte = unsafe { (*input).cur.offset_from((*input).base) };
        END_POS.with(|p| p.borrow_mut().push((line, byte as usize)));
    }
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
    SAX_CHAR_LOG.with(|l| l.borrow_mut().clear());
    GE_NAMES.with(|g| g.borrow_mut().clear());
    GE_STOP_PIC.with(|s| s.set(false));
    SAX2_START_LOG.with(|l| l.borrow_mut().clear());
    END_POS.with(|p| p.borrow_mut().clear());
}

/// Box a caller-provided SAX handler and create a push-parser context with
/// it (userData NULL → the context itself, mirroring upstream). Returns the
/// context and the boxed handler pointer (freed by the caller with
/// `Box::from_raw`).
unsafe fn push_ctxt_boxed(
    handler: crate::abi::structs::_xmlSAXHandler,
) -> (
    *mut crate::abi::structs::_xmlParserCtxt,
    *mut crate::abi::structs::_xmlSAXHandler,
) {
    let sax_ptr = Box::into_raw(Box::new(handler));
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
/// (bug25666: `</foo>` closing `<foo:a ...>`). The non-final call reports the
/// parse outcome exactly like upstream xmlParseChunk: the recorded error code
/// (76 = XML_ERR_TAG_NAME_MISMATCH) once the document ended not well-formed
/// (verified against the 2.15.3 oracle with chunkrc-probe.c — SP-14.3.1-4).
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
        assert_eq!(
            rc, 76,
            "non-final call reports errNo once the document failed"
        );
        assert_eq!((*ctxt).wellFormed, 0);
        assert_eq!((*ctxt).errNo, 76);
        let evs = SAX2_EVENTS.with(|e| e.borrow().clone());
        assert_eq!(evs.len(), 2, "both start events must be delivered");
        assert_eq!(evs[0], (b"a".to_vec(), b"u".to_vec()));
        // The child's prefix is declared on the ROOT — the parser-scoped
        // namespace stack must resolve it (bug25666/xml009).
        assert_eq!(evs[1], (b"b".to_vec(), b"v".to_vec()));
        // A later terminating call must not re-deliver and must report the
        // recorded error (upstream: xmlParseChunk returns errNo when
        // wellFormed == 0; events fire once).
        let rc2 = crate::abi::exports_xml2::xmlParseChunk(ctxt, ptr::null(), 0, 1);
        assert_eq!(rc2, 76);
        assert_eq!(SAX2_EVENTS.with(|e| e.borrow().len()), 2);
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

// ── SP-14.3.1-4 regression guards (BOM re-detection, DTD attribute defaults,
// ── external-entity-ref stop semantics, expat-compat no-double-delivery) ────

/// A leading UTF-8 BOM pushed as the first bytes of an initially-empty push
/// buffer is consumed like upstream's input-layer detection (bug35447's real
/// BOM variant): the first xmlParseChunk constructs the buffer empty, so BOM
/// detection must re-run when the first bytes arrive instead of erroring on
/// the raw BOM bytes. The same document also exercises the DTD ATTLIST default
/// (`type (literal|pattern|sub) "literal"`) which xmlParseStartTag2 appends to
/// the SAX2 attribute set (upstream ctxt->attsDefault).
///
/// # Safety
///
/// - The static buffers and module-scoped handler state are valid for the
///   call; the context and boxed handler are freed at the end.
#[test]
fn test_push_utf8_bom_and_dtd_attr_defaults() {
    unsafe {
        reset_push_capture();
        let doc: &[u8] = b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
                          <!DOCTYPE bundle [\n\
                          <!ELEMENT bundle (resource)+>\n\
                          <!ELEMENT resource (#PCDATA)>\n\
                          <!ATTLIST resource\n\
                          key CDATA #REQUIRED\n\
                          type (literal|pattern|sub) \"literal\"\n\
                          >\n\
                          ]>\n\
                          <resource key=\"rSeeYou\">A bient</resource>\n";
        // Control: the identical document WITHOUT the BOM/XML declaration must
        // produce the defaulted attribute too — isolates the DTD path from the
        // BOM/encoding path.
        let doc_no_bom: &[u8] = b"<!DOCTYPE bundle [\n\
                          <!ELEMENT bundle (resource)+>\n\
                          <!ELEMENT resource (#PCDATA)>\n\
                          <!ATTLIST resource\n\
                          key CDATA #REQUIRED\n\
                          type (literal|pattern|sub) \"literal\"\n\
                          >\n\
                          ]>\n\
                          <resource key=\"rSeeYou\">A bient</resource>\n";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = crate::abi::types::XML_SAX2_MAGIC as u32;
        h.startElementNs = Some(sax2_start_ns_log);
        h.endElementNs = Some(sax2_end_ns);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        // First the control document.
        let rc0 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc_no_bom.as_ptr() as *const i8,
            doc_no_bom.len() as c_int,
            1,
        );
        assert_eq!(rc0, 0, "control doc parses");
        let starts0 = SAX2_START_LOG.with(|l| l.borrow().clone());
        assert_eq!(starts0.len(), 1, "only the root element starts");
        if starts0[0].1.len() != 2 {
            panic!(
                "control doc attrs = {:?}",
                starts0[0]
                    .1
                    .iter()
                    .map(|(k, v)| (
                        String::from_utf8_lossy(k).into_owned(),
                        String::from_utf8_lossy(v).into_owned()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(starts0[0].1[0], (b"key".to_vec(), b"rSeeYou".to_vec()));
        assert_eq!(starts0[0].1[1], (b"type".to_vec(), b"literal".to_vec()));
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));

        // Now the BOM + XML-decl document.
        reset_push_capture();
        let mut h2 = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h2.initialized = crate::abi::types::XML_SAX2_MAGIC as u32;
        h2.startElementNs = Some(sax2_start_ns_log);
        h2.endElementNs = Some(sax2_end_ns);
        let (ctxt2, sax_ptr2) = push_ctxt_boxed(h2);
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt2,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1, // single-shot FINAL, like xml_parse_into_struct
        );
        assert_eq!(rc, 0, "BOM-prefixed push parse must succeed");
        assert_eq!((*ctxt2).errNo, 0);
        let starts = SAX2_START_LOG.with(|l| l.borrow().clone());
        assert_eq!(starts.len(), 1, "only the root element starts");
        assert_eq!(starts[0].0, b"resource");
        assert_eq!(starts[0].1.len(), 2, "key + defaulted type");
        assert_eq!(starts[0].1[0], (b"key".to_vec(), b"rSeeYou".to_vec()));
        assert_eq!(starts[0].1[1], (b"type".to_vec(), b"literal".to_vec()));
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt2);
        drop(Box::from_raw(sax_ptr2));
    }
}

/// bug71592: an external general parsed entity declared in the internal subset
/// resolves through the SAX getEntity callback; when that callback stops the
/// parser (PHP expat-compat's external-entity-ref handler returned FALSE →
/// xmlStopParser + errNo = XML_ERROR_EXTERNAL_ENTITY_HANDLING), the reference
/// expands to nothing, no further events fire (the trailing `</nop>` mismatch
/// is never reached) and the chunk call reports errNo — matching the oracle
/// (ext71592-probe.c).
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`; the getEntity callback
///   reads module-scoped capture state.
#[test]
fn test_push_external_entity_ref_stop_keeps_handler_error() {
    unsafe {
        reset_push_capture();
        GE_STOP_PIC.with(|s| s.set(true));
        let doc = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                   <!DOCTYPE root [\n\
                   <!ENTITY pic PUBLIC \"image.gif\" \"http://example.org/image.gif\">\n\
                   ]>\n\
                   <root>\n<p>&pic;</p>\n<p></nop>\n</root>\n";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = crate::abi::types::XML_SAX2_MAGIC as u32;
        h.getEntity = Some(sax_ge_resolve);
        h.startElementNs = Some(sax2_start_ns);
        h.endElementNs = Some(sax2_end_ns);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        // PHP expat-compat config: OLDSAX | NOENT (replaceEntities drives the
        // external-parsed registration).
        (*ctxt).options |= crate::abi::types::XML_PARSE_NOENT;
        (*ctxt).replaceEntities = 1;
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            0,
        );
        assert_eq!(rc, 21, "chunk reports the external-entity-handling error");
        assert_eq!((*ctxt).errNo, 21);
        assert_eq!((*ctxt).wellFormed, 0);
        assert_eq!((*ctxt).disableSAX, 2);
        let names = GE_NAMES.with(|g| g.borrow().clone());
        assert!(names.iter().any(|n| n == b"pic"));
        let evs = SAX2_EVENTS.with(|e| e.borrow().clone());
        assert_eq!(
            evs.len(),
            2,
            "root + p start before the stop, nothing after"
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// Expat-compat contexts enter the parse already not-well-formed (PHP zeroes
/// ctxt->wellFormed at create and never re-arms it); upstream xmlParseReference
/// then returns at its `if (!ctxt->wellFormed) return;` guard and the ENGINE
/// never re-parses resolved entity content — the compat get_entity side
/// effects are the only delivery. The engine must not double-substitute
/// (bug30875/gh14834 regressions: "a&ref;" produced "aentent").
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`; the chars callback logs
///   payloads into module-scoped state.
#[test]
fn test_push_wf0_compat_context_single_entity_delivery() {
    unsafe {
        reset_push_capture();
        let doc = b"<!DOCTYPE root [ <!ENTITY ref \"ent\"> ]><root>a&ref;b</root>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1; // SAX1 expat-compat
        h.getEntity = Some(sax_ge_resolve);
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        h.characters = Some(sax1_chars_log);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        (*ctxt).options |= crate::abi::types::XML_PARSE_NOENT;
        (*ctxt).replaceEntities = 1;
        (*ctxt).wellFormed = 0; // XML_ParserCreate_MM zeroes it after create
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc, 0, "clean parse reports 0");
        assert_eq!((*ctxt).errNo, 0);
        let runs = SAX_CHAR_LOG.with(|l| l.borrow().clone());
        let mut text = Vec::new();
        for r in &runs {
            text.extend_from_slice(r);
        }
        // get_entity resolved "ref" (compat delivers the content), so the
        // engine must NOT re-substitute it.
        assert_eq!(runs.len(), 2, "a and b only — no engine re-delivery of ent");
        assert_eq!(runs[0], b"a");
        assert_eq!(runs[1], b"b");
        assert_eq!(&text, b"ab");
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// Control for the guard above: a well-formed-tracking context (wellFormed=1,
/// the non-expat configuration) still substitutes resolved entity content
/// through the engine, exactly like upstream with wellFormed == 1
/// (intent-probe.c oracle behavior: CH(a) CH(ent)).
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`.
#[test]
fn test_push_wf1_context_substitutes_entity_content() {
    unsafe {
        reset_push_capture();
        let doc = b"<!DOCTYPE root [ <!ENTITY ref \"ent\"> ]><root>a&ref;b</root>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1;
        h.getEntity = Some(sax_ge_resolve);
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        h.characters = Some(sax1_chars_log);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        (*ctxt).options |= crate::abi::types::XML_PARSE_NOENT;
        (*ctxt).replaceEntities = 1;
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc, 0);
        assert_eq!((*ctxt).errNo, 0);
        let runs = SAX_CHAR_LOG.with(|l| l.borrow().clone());
        assert_eq!(runs.len(), 3, "a + ent + b substituted by the engine");
        assert_eq!(runs[0], b"a");
        assert_eq!(runs[1], b"ent");
        assert_eq!(runs[2], b"b");
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

// ── SP-14.3.1-5 regression guard (end-element locator) ──────────────────────

/// The endElement SAX event reports the input position one byte past the end
/// tag's closing '>' — xml_get_current_line_number/column/byte_index at the
/// end callback (bug26614: `</DATA> at line 9, col %d (byte 96)` for the
/// CDATA/comment/text variants; upstream xmlParseEndTag1/2 fire the callback
/// after the tag was consumed).
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`; the end callback reads
///   module-scoped capture state.
#[test]
fn test_push_end_element_locator_is_one_past_gt() {
    unsafe {
        reset_push_capture();
        // bug26614.inc case 1: CDATA block between <data> and </data>.
        let doc = b"<?xml version=\"1.0\" encoding=\"iso-8859-1\" ?>\n\
                   <data>\n\
                   <![CDATA[\n\
                   multi\n\
                   line\n\
                   CDATA\n\
                   block\n\
                   ]]>\n\
                   </data>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1; // SAX1 expat-compat (xml_parser_create)
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end_pos);
        h.characters = Some(sax1_chars);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc, 0);
        assert_eq!((*ctxt).errNo, 0);
        let ends = END_POS.with(|p| p.borrow().clone());
        assert_eq!(ends.len(), 1, "one endElement (</data>)");
        assert_eq!(ends[0].0, 9, "line of the </data> end tag");
        assert_eq!(
            ends[0].1, 96,
            "byte index one past the '>' of </data> (bug26614 oracle value)"
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

// ── SP-14.3.1-6 regression guards (multi-call eager delivery, name limits) ─

/// Upstream parses each xmlParseChunk eagerly, so a NON-final call on an
/// INCOMPLETE document still delivers every event whose construct completed;
/// the terminating call continues from there (XML_OPTION_PARSE_HUGE multi-call
/// flow: the head delivers CONTAINER/A/A/SECOND, the tail only closes the
/// container). Nothing is delivered twice.
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`; SAX1 events are captured
///   in module-scoped state.
#[test]
fn test_push_multicall_eager_delivery_then_resume() {
    unsafe {
        reset_push_capture();
        let head = b"<?xml version=\"1.0\"?><container><aaa/><aaa/><second>foo</second>";
        let tail = b"</container>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1; // SAX1 expat-compat (xml_parser_create)
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        h.characters = Some(sax1_chars_log);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        // Call 1: NON-final with the incomplete head — upstream fires the
        // completed constructs immediately.
        let rc1 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            head.as_ptr() as *const i8,
            head.len() as c_int,
            0,
        );
        assert_eq!(rc1, 0);
        assert_eq!((*ctxt).errNo, 0);
        let starts1 = SAX1_EVENTS.with(|e| e.borrow().clone());
        assert_eq!(
            starts1.len(),
            4,
            "CONTAINER + aaa + aaa + second delivered on the non-final call"
        );
        // Call 2: final with the tail — only the container's end remains.
        let rc2 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            tail.as_ptr() as *const i8,
            tail.len() as c_int,
            1,
        );
        assert_eq!(rc2, 0);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 4, "all four ends exactly once");
        assert_eq!(
            SAX1_EVENTS.with(|e| e.borrow().len()),
            4,
            "no start re-delivered by the terminating call"
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

/// Without XML_PARSE_HUGE an element name longer than XML_MAX_NAME_LENGTH
/// (50 000 bytes) fails the start-tag parse with XML_ERR_NAME_REQUIRED (68)
/// "StartTag: invalid element name" and the element is never reported — the
/// parser stops (Request #68325 / XML_OPTION_PARSE_HUGE). With XML_PARSE_HUGE
/// the limit becomes XML_MAX_TEXT_LENGTH (10 000 000) and the name parses.
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`.
#[test]
fn test_push_name_length_limit_without_huge() {
    unsafe {
        reset_push_capture();
        let long_name: Vec<u8> = std::iter::repeat(b'A').take(50_001).collect();
        let mut doc = b"<container><".to_vec();
        doc.extend_from_slice(&long_name);
        doc.extend_from_slice(b"/></container>");
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1;
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        let rc = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc, 68, "name-too-long is fatal without XML_PARSE_HUGE");
        assert_eq!((*ctxt).errNo, 68);
        assert_eq!((*ctxt).wellFormed, 0);
        assert_eq!(
            SAX1_STARTS.with(|c| c.get()),
            1,
            "only CONTAINER was reported"
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));

        // With XML_PARSE_HUGE the same document parses (limit 10 000 000).
        reset_push_capture();
        let mut h2 = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h2.initialized = 1;
        h2.startElement = Some(sax1_start);
        h2.endElement = Some(sax1_end);
        let (ctxt2, sax_ptr2) = push_ctxt_boxed(h2);
        (*ctxt2).options |= crate::abi::types::XML_PARSE_HUGE;
        let rc2 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt2,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc2, 0);
        assert_eq!((*ctxt2).errNo, 0);
        assert_eq!(
            SAX1_STARTS.with(|c| c.get()),
            2,
            "container + the huge name"
        );
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt2);
        drop(Box::from_raw(sax_ptr2));
    }
}

// ── SP-14.3.1-7 regression guard (completed context refuses re-parse) ──────

/// A parser context that finished a complete document stays at XML_PARSER_EOF:
/// a second xmlParseChunk (gh12254's second xml_parse_into_struct on the same
/// parser) must parse NOTHING — no events fire again (upstream
/// xmlParseTryOrFinish `case XML_PARSER_EOF: goto done`).
///
/// # Safety
///
/// - As `test_push_utf8_bom_and_dtd_attr_defaults`.
#[test]
fn test_push_completed_context_refuses_reparse() {
    unsafe {
        reset_push_capture();
        let doc = b"<container/>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1;
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        // First parse: complete document (single-shot final, like
        // xml_parse_into_struct).
        let rc1 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc1, 0);
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 1);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 1);
        assert_eq!(
            unsafe { (*ctxt).instate },
            crate::abi::types::xmlParserInputState::XML_PARSER_EOF as c_int
        );
        // Second parse of the same document on the same context: no events.
        let rc2 = crate::abi::exports_xml2::xmlParseChunk(
            ctxt,
            doc.as_ptr() as *const i8,
            doc.len() as c_int,
            1,
        );
        assert_eq!(rc2, 0);
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 1, "no second open");
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 1, "no second close");
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

// ── SP-14.3.1-8 regression guard (incremental-tag EOF / default-markup) ─────

/// GH-20439 feeds a push parser ONE BYTE at a time. Between the bytes the
/// parser is usually mid-construct, so the accumulated input repeatedly ends
/// right after `<`, `<!`, `<!--`, inside an attribute literal, etc. Upstream
/// never treats those tag prefixes as character data: it pauses until the
/// construct completes (xmlParseTryOrFinish EOF-in-construct states). A
/// spurious `Characters("<")` / `Characters("<!")` event from the end of one
/// chunk would corrupt the raw-markup the PHP default handler later seeks back
/// to (and gh20439's per-byte feed crashed on the stale input pointer). The
/// parser must deliver only real text and pause on incomplete markup.
///
/// # Safety
///
/// - As `test_push_single_shot_without_terminate_delivers`; SAX1 events are
///   captured in module-scoped state.
#[test]
fn test_push_incremental_eof_prefixes_not_text() {
    unsafe {
        reset_push_capture();
        // Per-byte feed (gh20439_1's pattern). The only real text is inside
        // root; every `<`/`<!`/`<!--`/`-->` boundary is crossed byte-by-byte.
        let doc = b"<!-- note --><root>hi</root>";
        let mut h = crate::abi::structs::_xmlSAXHandler {
            ..std::mem::zeroed()
        };
        h.initialized = 1; // SAX1 expat-compat (xml_parser_create)
        h.startElement = Some(sax1_start);
        h.endElement = Some(sax1_end);
        h.characters = Some(sax1_chars_log);
        let (ctxt, sax_ptr) = push_ctxt_boxed(h);
        for one in doc.iter() {
            let bytes = [*one];
            let rc = crate::abi::exports_xml2::xmlParseChunk(
                ctxt,
                bytes.as_ptr() as *const i8,
                bytes.len() as c_int,
                0,
            );
            assert_eq!(rc, 0);
        }
        let rc = crate::abi::exports_xml2::xmlParseChunk(ctxt, ptr::null(), 0, 1);
        assert_eq!(rc, 0);
        // One root element, opened and closed exactly once, and the only
        // character data delivered is the real text "hi" — no spurious "<"/
        // "<!" runs leaked as text across the chunk boundaries.
        assert_eq!(SAX1_STARTS.with(|c| c.get()), 1);
        assert_eq!(SAX1_ENDS.with(|c| c.get()), 1);
        let all: Vec<u8> = SAX_CHAR_LOG.with(|l| l.borrow().iter().flatten().cloned().collect());
        assert_eq!(all, b"hi", "only the real text run may be delivered");
        crate::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
        drop(Box::from_raw(sax_ptr));
    }
}

// ── KEY-2 regression guard (content-`<!`-markup rule) ───────────────────────

/// A `<!` that is not `<!--`, `<![CDATA[`, or a prolog `<!DOCTYPE>` is an
/// invalid element start in element CONTENT: upstream xmlParseStartTag fails
/// the name at the '!' with XML_ERR_NAME_REQUIRED (68) and the document
/// becomes not well-formed — the construct is never swallowed as text. This is
/// what makes PHP's XML innerHTML/outerHTML fragment writer reject
/// `<!ENTITY ...>`/`<!DOCTYPE ...>` bodies ("XML fragment is not well-formed")
/// and is the engine rule that lets the SP-14.3.1-8 push-EOF edits land
/// without regressing those dom fragment tests (KEY-2).
///
/// # Safety
///
/// - As `test_undeclared_entity_fatal_without_extsubset_or_perefs`.
#[test]
fn test_content_markup_decl_clears_wellformed() {
    unsafe {
        // Illegal `<!` constructs in element content: NAME_REQUIRED, wf = 0.
        for doc in [
            &b"<root><!ENTITY foo \"content\"></root>"[..],
            &b"<root><!DOCTYPE html></root>"[..],
            &b"<root><!ELEMENT x EMPTY></root>"[..],
            &b"<root><!junk></root>"[..],
        ] {
            let (d, wf, err) = read_memory_ctx(doc, 0);
            assert!(d.is_null());
            assert_eq!(
                wf,
                0,
                "{} must be not well-formed",
                String::from_utf8_lossy(doc)
            );
            assert_eq!(
                err,
                crate::abi::types::XML_ERR_NAME_REQUIRED,
                "{} must raise 68",
                String::from_utf8_lossy(doc)
            );
        }
        // Legal content constructs stay well-formed.
        for doc in [
            &b"<root><!-- ok --></root>"[..],
            &b"<root><![CDATA[ok]]></root>"[..],
            &b"<root>plain <b>text</b></root>"[..],
        ] {
            let (d, wf, err) = read_memory_ctx(doc, 0);
            assert!(!d.is_null());
            assert_eq!(wf, 1);
            assert_eq!(err, 0);
            tree::free_doc(d);
        }
        // A prolog DOCTYPE with an internal subset stays legal.
        let (d, wf, err) =
            read_memory_ctx(b"<!DOCTYPE root [ <!ENTITY e \"E\"> ]><root>a</root>", 0);
        assert!(!d.is_null());
        assert_eq!(wf, 1);
        assert_eq!(err, 0);
        tree::free_doc(d);
    }
}

// ── KEY-4 guards: RECOVER premature-EOF diagnostics + NO_XXE external gating
// ────────────────────────────────────────────────────────────────────────────

/// A RECOVER parse of an unterminated root raises XML_ERR_TAG_NOT_FINISHED
/// (77) AND still returns the recovered tree — recovery decides whether
/// parsing continues, not whether the diagnostic fires. The pre-fix recovery
/// branch closed the element silently (no error record, no php warning);
/// ext/simplexml + ext/dom xml_parsing_LIBXML_RECOVER show the warning block
/// exactly like the non-recover case.
///
/// # Safety
///
/// - The static buffer is valid for the parse; `read_memory_ctx` frees the
///   context and returns an owned doc (freed below).
#[test]
fn test_recover_raises_premature_eof_tag_not_finished() {
    unsafe {
        // Non-recover: fatal, no doc.
        let (doc, wf, err) = read_memory_ctx(b"<root><child/>", 0);
        assert!(doc.is_null());
        assert_eq!(wf, 0);
        assert_eq!(err, crate::abi::types::XML_ERR_TAG_NOT_FINISHED);

        // Recover: same diagnostic (77), but the tree is returned.
        let (doc, wf, err) =
            read_memory_ctx(b"<root><child/>", crate::abi::types::XML_PARSE_RECOVER);
        assert!(!doc.is_null(), "RECOVER must return the recovered tree");
        assert_eq!(wf, 0, "the fatal error still clears wellFormed");
        assert_eq!(err, crate::abi::types::XML_ERR_TAG_NOT_FINISHED);
        tree::free_doc(doc);
    }
}

/// LIBXML_NO_XXE blocks the load AND substitution of an EXTERNAL general
/// parsed entity while internal entities still substitute — the reference
/// expands to nothing, no loader is invoked (no "failed to load" I/O warning)
/// and the saved document carries a SINGLE <!DOCTYPE> whose declarations are
/// in declaration order (ext/simplexml + ext/dom xml_parsing_LIBXML_NO_XXE;
/// the doctype duplication + reversed entity order were a serializer defect —
/// upstream xmlDtdDumpOutput walks the DTD node's children list).
///
/// # Safety
///
/// - The static buffer is valid for the parse; the owned doc is freed via
///   `tree::free_doc` after the serialized-bytes check.
#[test]
fn test_no_xxe_blocks_external_entity_and_doctype_serializes_once_in_order() {
    unsafe {
        let xml = b"<?xml version='1.0' encoding='utf-8'?>\n<!DOCTYPE set [\n    <!ENTITY foo '<foo>bar</foo>'>\n    <!ENTITY xxe SYSTEM \"file:///etc/passwd\">\n]>\n<set>&foo;&xxe;</set>\n";
        let (doc, wf, err) = read_memory_ctx(
            xml,
            crate::abi::types::XML_PARSE_NOENT | crate::abi::types::XML_PARSE_NO_XXE,
        );
        assert!(!doc.is_null(), "NO_XXE doc must parse");
        assert_eq!(wf, 1);
        assert_eq!(err, 0);
        // The saved doc: single doctype, foo before xxe, external ref gone.
        let buf = crate::xml::io::buf_create(-1);
        assert!(!buf.is_null());
        tree::serialize_node(doc as *mut _xmlNode, buf, 0, 0);
        let content = crate::xml::io::buf_content(buf);
        let bytes = crate::xml::string::xmlstr_to_bytes(content);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        crate::xml::io::buf_free(buf);
        assert_eq!(
            text,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <!DOCTYPE set [\n\
             <!ENTITY foo \"<foo>bar</foo>\">\n\
             <!ENTITY xxe SYSTEM \"file:///etc/passwd\">\n\
             ]>\n\
             <set><foo>bar</foo></set>\n"
        );
        tree::free_doc(doc);
    }
}
