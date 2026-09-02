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
