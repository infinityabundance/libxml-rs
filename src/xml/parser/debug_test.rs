//! Parser-internal debug/tokenizer tests (§85 Phase 3).
//!
//! # Upstream contract
//!
//! These unit tests exercise the tokenizer and input-stack plumbing that
//! underpin the parser state machine (upstream parser.c/parserInternals.c).
//! They exist to catch regressions in the internal pipeline; the
//! byte-identical CLI corpus (CLI-XMLLINT-*) is the authoritative oracle
//! parity evidence, while these tests pin the Rust-internal behavior.
//!
//! # Conceptual behavior
//!
//! The tests drive `XmlTokenizer` / `InputStack` / `InputBuffer` directly
//! over small XML snippets (elements, attributes, namespaces, entities) and
//! assert the resulting token streams and buffer states — the same inputs
//! the state machine consumes when parsing real documents.
//!
//! # Historical quirks & epochs
//!
//! Parser diagnostic counts and caret positions are epoch-pinned
//! observables (E-002/E-005 in SEMANTIC_EPOCHS.md); the tokenizer-level
//! expectations here intentionally follow the current 2.15.3 epoch and
//! must be updated together with the state machine when an epoch boundary
//! is crossed.
//!
//! # Deliberate oddities
//!
//! The tests are compiled only under `#[cfg(test)]`; they are not part of
//! the published crate surface and must never be reached from the ABI.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to drop these tests because the CLI corpus
//! already covers the parser. The corpus runs whole documents end-to-end;
//! these tests isolate single tokenizer/input behaviors (e.g. BOM
//! handling, buffer refill) that a document-level mismatch would be slow
//! to attribute. Keep them as the first-level bisection aid.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod debug_test {

    use crate::xml::parser::helpers;
    use crate::xml::parser::input::InputBuffer;
    use crate::xml::parser::input::InputStack;
    use crate::xml::parser::tokenizer::{XmlToken, XmlTokenizer};

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
                attr_end,
                attr_start,
                end_pos: _,
                empty,
                unterminated,
            } => {
                assert_eq!(name.as_slice(), b"root");
                assert!(attributes.is_empty());
                assert!(attr_end.is_empty());
                assert!(attr_start.is_empty());
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

    /// Regression court (11.1-X R-000165): `ctxt._private` is application
    /// data (upstream `xmlCtxtSetPrivate`/`xmlCtxtGetPrivate`). The internal
    /// parse-input stash lives in a side table, so setting the private field
    /// and freeing the context must never free the application's pointer as
    /// an `InputBuffer` (previously a double-free/UB).
    #[test]
    fn test_ctxt_private_is_application_data() {
        unsafe {
            let ctxt = helpers::create_parser_ctxt();
            assert!(!ctxt.is_null());
            // A stack marker that is NOT a Box<InputBuffer>.
            let marker: usize = 0x1234_5678;
            crate::abi::exports_parserint::xmlCtxtSetPrivate(
                ctxt,
                marker as *mut core::ffi::c_void,
            );
            assert_eq!(
                crate::abi::exports_parserint::xmlCtxtGetPrivate(ctxt) as usize,
                marker
            );
            // Freeing must not interpret the marker as an internal buffer.
            helpers::free_parser_ctxt(ctxt);

            // And with a stashed parse input present, private stays intact:
            let ctxt = helpers::create_parser_ctxt();
            let input = helpers::input_from_memory(c"<a/>".as_ptr(), 5);
            helpers::setup_parser_input(ctxt, input);
            crate::abi::exports_parserint::xmlCtxtSetPrivate(
                ctxt,
                marker as *mut core::ffi::c_void,
            );
            assert_eq!(
                crate::abi::exports_parserint::xmlCtxtGetPrivate(ctxt) as usize,
                marker
            );
            helpers::free_parser_ctxt(ctxt);
        }
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
                attr_end,
                attr_start,
                end_pos: _,
                empty,
                unterminated,
            } => {
                assert_eq!(name.as_slice(), b"root");
                assert_eq!(attributes.len(), 1);
                assert_eq!(attr_end.len(), 1);
                assert_eq!(attr_start.len(), 1);
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

            // Take ownership of the stashed InputBuffer and create XmlParser
            // directly (the stash side table, not ctxt._private — 11.1-X).
            let input_buf_ptr = helpers::take_stashed_input_buffer(ctxt);
            assert!(!input_buf_ptr.is_null(), "input_buf_ptr is null");
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

            let input_buf_ptr = helpers::take_stashed_input_buffer(ctxt);
            assert!(!input_buf_ptr.is_null(), "input_buf_ptr is null");
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
