#![no_main]

use libfuzzer_sys::fuzz_target;
use std::os::raw::{c_char, c_int};
use std::ptr;

/// Coverage-guided fuzz of the XML parser entry point. Finds panics and
/// memory-safety bugs (with `-Z sanitizer=address`) that the proptest smoke
/// in `src/fuzz.rs` cannot.
fuzz_target!(|data: &[u8]| {
    let mut buf = Vec::with_capacity(data.len() + 1);
    buf.extend_from_slice(data);
    buf.push(0);

    let doc = unsafe {
        libxml_rs::abi::exports_xml2::xmlReadMemory(
            buf.as_ptr() as *const c_char,
            data.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if !doc.is_null() {
        unsafe { libxml_rs::abi::exports_xml2::xmlFreeDoc(doc) };
    }
});
