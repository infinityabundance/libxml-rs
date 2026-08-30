#[cfg(test)]
mod debug_test {
    use crate::abi::structs::*;
    use crate::xml::parser::helpers;
    use crate::xml::parser::input::InputBuffer;
    use crate::xml::parser::input::InputStack;
    use crate::xml::parser::tokenizer::{XmlToken, XmlTokenizer};
    use core::ptr;

    #[test]
    fn test_tokenizer_simple() {
        let data = b"<root/>";
        let buf = InputBuffer::from_memory(data, None);
        let stack = InputStack::new(buf);
        let mut tok = XmlTokenizer::new(stack);

        let token = tok.next_token();
        match &token {
            XmlToken::StartTag {
                name,
                attributes,
                empty,
                unterminated,
            } => {
                assert_eq!(name.as_slice(), b"root");
                assert!(attributes.is_empty());
                assert!(*empty);
                assert!(!*unterminated);
            }
            other => panic!("Expected StartTag, got {:?}", other),
        }

        let token = tok.next_token();
        assert_eq!(token, XmlToken::Eof);
    }

    #[test]
    fn test_tokenizer_with_text() {
        let data = b"<root>Hello</root>";
        let buf = InputBuffer::from_memory(data, None);
        let stack = InputStack::new(buf);
        let mut tok = XmlTokenizer::new(stack);

        let token = tok.next_token();
        assert!(matches!(token, XmlToken::StartTag { .. }));

        let token = tok.next_token_raw();
        assert!(matches!(token, XmlToken::Characters(_)));
        if let XmlToken::Characters(text) = &token {
            assert_eq!(text.as_slice(), b"Hello");
        }

        let token = tok.next_token_raw();
        assert!(matches!(token, XmlToken::EndTag { .. }));
    }

    #[test]
    fn test_tokenizer_complex() {
        let data = b"<?xml version=\"1.0\"?>\n<root id=\"123\"/>\n";
        let buf = InputBuffer::from_memory(data, None);
        let stack = InputStack::new(buf);
        let mut tok = XmlTokenizer::new(stack);

        let token = tok.next_token();
        assert!(matches!(token, XmlToken::XmlDecl { .. }));

        let token = tok.next_token();
        match &token {
            XmlToken::StartTag {
                name,
                attributes,
                empty,
                unterminated,
            } => {
                assert_eq!(name.as_slice(), b"root");
                assert_eq!(attributes.len(), 1);
                assert!(*empty);
                assert!(!*unterminated);
            }
            other => panic!("Expected StartTag, got {:?}", other),
        }
    }

    #[test]
    fn test_input_buffer_basic() {
        let data = b"<root/>";
        let mut buf = InputBuffer::from_memory(data, None);

        assert_eq!(buf.read_char(), Some('<'));
        assert_eq!(buf.read_char(), Some('r'));
        assert_eq!(buf.read_char(), Some('o'));
        assert_eq!(buf.read_char(), Some('o'));
        assert_eq!(buf.read_char(), Some('t'));
        assert_eq!(buf.read_char(), Some('/'));
        assert_eq!(buf.read_char(), Some('>'));
        assert!(buf.is_eof());
    }

    #[test]
    fn test_parser_simple_el() {
        unsafe {
            // Directly test the internal parser with XmlParser
            let xml = b"<root/>";
            let ctxt = helpers::create_parser_ctxt();
            assert!(!ctxt.is_null(), "ctxt is null");

            let input = helpers::input_from_memory(xml.as_ptr() as *const i8, xml.len() as i32);
            helpers::setup_parser_input(ctxt, input);

            assert!(!(*ctxt).input.is_null(), "ctxt.input is null");
            assert!(!(*ctxt).sax.is_null(), "ctxt.sax is null");
            assert_eq!(
                (*(*ctxt).sax).initialized,
                crate::abi::types::XML_SAX2_MAGIC as u32
            );

            // Take ownership of the InputBuffer and create XmlParser directly
            let input_buf_ptr = (*ctxt)._private as *mut InputBuffer;
            assert!(!input_buf_ptr.is_null(), "input_buf_ptr is null");
            (*ctxt)._private = ptr::null_mut();
            let input_buf = Box::from_raw(input_buf_ptr);
            let input_stack = InputStack::new(*input_buf);

            let mut parser = crate::xml::parser::state::XmlParser::new(input_stack, ctxt);

            // Before parsing, verify SAX handler is set up
            let sax = &*(*ctxt).sax;
            assert!(
                sax.startDocument.is_some(),
                "startDocument callback not set"
            );

            let ret = parser.parse_document();
            eprintln!("parse_document returned: {}", ret);

            let doc = (*ctxt).myDoc;
            eprintln!("doc is null: {}", doc.is_null());

            if !doc.is_null() {
                eprintln!("doc.children is null: {}", (*doc).children.is_null());
                if !(*doc).children.is_null() {
                    let name = crate::xml::string::xmlstr_to_bytes(
                        (*(*doc).children).name as *const crate::abi::types::xmlChar,
                    );
                    eprintln!("root name: {:?}", name);
                }
            }

            assert!(!doc.is_null(), "doc should not be null, ret={}", ret);
        }
    }

    #[test]
    fn test_parser_with_text() {
        unsafe {
            let xml = b"<root>Hello</root>";
            let ctxt = helpers::create_parser_ctxt();
            assert!(!ctxt.is_null());

            let input = helpers::input_from_memory(xml.as_ptr() as *const i8, xml.len() as i32);
            helpers::setup_parser_input(ctxt, input);

            let input_buf_ptr = (*ctxt)._private as *mut InputBuffer;
            (*ctxt)._private = ptr::null_mut();
            let input_buf = Box::from_raw(input_buf_ptr);
            let input_stack = InputStack::new(*input_buf);

            let mut parser = crate::xml::parser::state::XmlParser::new(input_stack, ctxt);
            let ret = parser.parse_document();
            eprintln!("parse_document returned: {}", ret);

            let doc = (*ctxt).myDoc;
            eprintln!("doc is null: {}", doc.is_null());

            assert!(!doc.is_null(), "doc should not be null, ret={}", ret);

            if !doc.is_null() {
                let root = (*doc).children;
                assert!(!root.is_null(), "root should not be null");
                eprintln!(
                    "root name: {:?}",
                    crate::xml::string::xmlstr_to_bytes(
                        (*root).name as *const crate::abi::types::xmlChar
                    )
                );
            }
        }
    }
}
