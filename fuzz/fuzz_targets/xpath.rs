#![no_main]

use libfuzzer_sys::fuzz_target;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// Coverage-guided fuzz of the XPath evaluator against a small document.
/// The first byte selects a seed document; the remainder is the expression.
fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    const DOCS: &[&[u8]] = &[
        b"<root/>\0",
        b"<root><a><b>x</b></a><c/></root>\0",
        b"<r xmlns:n=\"urn:n\"><n:a id=\"1\">text</n:a></r>\0",
    ];
    let doc_bytes = DOCS[(data[0] as usize) % DOCS.len()];
    let expr = &data[1..];

    let mut expr_buf = Vec::with_capacity(expr.len() + 1);
    expr_buf.extend_from_slice(expr);
    expr_buf.push(0);

    let doc = unsafe {
        libxml_rs::abi::exports_xml2::xmlReadMemory(
            doc_bytes.as_ptr() as *const c_char,
            (doc_bytes.len() - 1) as c_int,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if doc.is_null() {
        return;
    }
    unsafe {
        let ctx = libxml_rs::abi::exports_xml2::xmlXPathNewContext(doc);
        let obj = libxml_rs::abi::exports_xml2::xmlXPathEvalExpression(
            expr_buf.as_ptr() as *const u8,
            ctx,
        );
        if !obj.is_null() {
            libxml_rs::abi::exports_xml2::xmlXPathFreeObject(obj);
        }
        libxml_rs::abi::exports_xml2::xmlXPathFreeContext(ctx);
        libxml_rs::abi::exports_xml2::xmlFreeDoc(doc);
    }
});
