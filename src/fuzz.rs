//! Property-based fuzzing smoke targets (proptest).
//!
//! These run in `cargo test` on stable Rust and feed arbitrary byte strings
//! into the public parser entry points, asserting the drop-in never panics on
//! hostile input. They are a CI-friendly complement to the coverage-guided
//! libFuzzer targets in `fuzz/` (which require nightly + ASan and find the
//! memory-safety class of bugs a `catch_unwind` smoke cannot).

use std::os::raw::{c_char, c_int};
use std::ptr;

use proptest::prelude::*;

/// Feed `data` (NUL-terminated) to `xmlReadMemory` and free any document.
fn drive_xml(data: &[u8]) {
    let mut buf = Vec::with_capacity(data.len() + 1);
    buf.extend_from_slice(data);
    buf.push(0);
    let doc = unsafe {
        crate::abi::exports_xml2::xmlReadMemory(
            buf.as_ptr() as *const c_char,
            data.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if !doc.is_null() {
        unsafe { crate::abi::exports_xml2::xmlFreeDoc(doc) };
    }
}

fn drive_html(data: &[u8]) {
    let mut buf = Vec::with_capacity(data.len() + 1);
    buf.extend_from_slice(data);
    buf.push(0);
    let doc = unsafe {
        crate::abi::exports_html::htmlReadMemory(
            buf.as_ptr() as *const c_char,
            data.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if !doc.is_null() {
        unsafe { crate::abi::exports_xml2::xmlFreeDoc(doc) };
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 2000,
        ..ProptestConfig::default()
    })]

    /// Arbitrary byte strings never panic the XML parser.
    #[test]
    fn fuzz_xml_read_memory_no_panic(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        drive_xml(&data);
    }

    /// Arbitrary byte strings never panic the HTML parser.
    #[test]
    fn fuzz_html_read_memory_no_panic(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        drive_html(&data);
    }
}
