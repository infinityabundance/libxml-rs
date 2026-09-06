//! §16.5.2 allocation probe — proves the borrowed-input path performs no
//! Rust-side input copy while the owned (multi-phase) path does one.
//!
//! A `#[global_allocator]` counts every Rust allocation (the input copy is a
//! `Vec` copy; tree content flows through the C allocator hooks and is NOT
//! counted). Run:
//!
//! ```sh
//! cargo run --release --example alloc_probe
//! ```
//!
//! Expected shape for an N-byte document:
//! - `xmlReadMemory`/`xmlParseMemory` (borrowed front-ends): ≈ C + tokenizer
//!   staging — NO N-byte input copy.
//! - `xmlCtxtReadMemory` on a caller-owned context (owned front-end): ≈ the
//!   same + N (one input copy, exactly like upstream, which also copies).

use std::alloc::{GlobalAlloc, Layout, System};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;
static COUNTED: AtomicU64 = AtomicU64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        COUNTED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        if new_size > layout.size() {
            COUNTED.fetch_add((new_size - layout.size()) as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn doc_bytes(n: usize) -> Vec<u8> {
    let mut d = Vec::with_capacity(n);
    d.extend_from_slice(b"<r>");
    d.resize(n - 7, b'x');
    d.extend_from_slice(b"</r>");
    debug_assert_eq!(d.len(), n);
    d
}

fn snapshot(tag: &str, before: u64, before_a: u64) {
    let after = COUNTED.load(Ordering::Relaxed);
    let after_a = ALLOCS.load(Ordering::Relaxed);
    println!(
        "{tag}: rust-alloc-bytes={} rust-allocs={}",
        after - before,
        after_a - before_a
    );
}

fn main() {
    // Warm the allocator / lazy statics outside the windows.
    let warm = doc_bytes(1 << 16);
    unsafe {
        let ctxt = libxml_rs::abi::exports_parser::xmlNewParserCtxt();
        let d = libxml_rs::abi::exports_xml2::xmlReadMemory(
            warm.as_ptr() as *const c_char,
            warm.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        );
        if !d.is_null() {
            libxml_rs::abi::exports_xml2::xmlFreeDoc(d);
        }
        libxml_rs::abi::exports_xml2::xmlFreeParserCtxt(ctxt);
    }
    drop(warm);

    for n in [1usize << 20, 2 << 20, 4 << 20] {
        let doc = doc_bytes(n);
        println!("== input size {n} bytes ==");

        // xmlReadMemory — one-call front-end: BORROWED (no input copy).
        let before = COUNTED.load(Ordering::Relaxed);
        let before_a = ALLOCS.load(Ordering::Relaxed);
        let d = unsafe {
            libxml_rs::abi::exports_xml2::xmlReadMemory(
                doc.as_ptr() as *const c_char,
                doc.len() as c_int,
                ptr::null(),
                ptr::null(),
                0,
            )
        };
        snapshot("xmlReadMemory   (borrowed)", before, before_a);
        assert!(!d.is_null());
        unsafe { libxml_rs::abi::exports_xml2::xmlFreeDoc(d) };

        // xmlCtxtReadMemory — caller-owned persistent context: OWNED
        // (one input copy, upstream parity).
        let keep = unsafe { libxml_rs::abi::exports_parser::xmlNewParserCtxt() };
        let before = COUNTED.load(Ordering::Relaxed);
        let before_a = ALLOCS.load(Ordering::Relaxed);
        let d2 = unsafe {
            libxml_rs::abi::exports_parser::xmlCtxtReadMemory(
                keep,
                doc.as_ptr() as *const c_char,
                doc.len() as c_int,
                ptr::null(),
                ptr::null(),
                0,
            )
        };
        snapshot("xmlCtxtReadMemory(owned)   ", before, before_a);
        assert!(!d2.is_null());
        unsafe {
            libxml_rs::abi::exports_xml2::xmlFreeDoc(d2);
            libxml_rs::abi::exports_xml2::xmlFreeParserCtxt(keep);
        }
        drop(doc);
    }
}
