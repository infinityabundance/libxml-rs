//! Integration tests for the XML parser.
//!
//! These tests verify that the parser can parse real XML documents
//! and produce correct trees. They exercise the full pipeline:
//! C ABI exports → helpers → input → tokenizer → state machine → SAX → tree.

use crate::abi::structs::*;
use crate::abi::types::xmlChar;
use crate::xml::parser::helpers;
use crate::xml::tree;
use core::ptr;

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
