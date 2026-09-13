//! §16.8 measurement harness (temporary): time `xmlReadMemory` on a file,
//! reporting ns/parse and MB/s. Used to bound the byte-scanning fraction by
//! comparing `LIBXML_RS_SCAN_BACKEND=scalar` vs `avx2`/`avx512`.
//!
//! Usage: cargo run --release --example scanbench -- <file> [iters]

use std::os::raw::{c_char, c_int};
use std::ptr;
use std::time::Instant;

use libxml_rs::abi::exports_xml2::{xmlFreeDoc, xmlReadMemory};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: scanbench <file> [iters]");
    let iters: usize = args.next().map(|s| s.parse().unwrap()).unwrap_or(20);
    let data = std::fs::read(&path).expect("read file");

    // Warm up + sanity.
    unsafe {
        let d = xmlReadMemory(
            data.as_ptr() as *const c_char,
            data.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        );
        assert!(!d.is_null(), "parse failed");
        xmlFreeDoc(d);
    }

    let mut best = f64::MAX;
    for _ in 0..iters {
        let t = Instant::now();
        unsafe {
            let d = xmlReadMemory(
                data.as_ptr() as *const c_char,
                data.len() as c_int,
                ptr::null(),
                ptr::null(),
                0,
            );
            assert!(!d.is_null());
            xmlFreeDoc(d);
        }
        let s = t.elapsed().as_secs_f64();
        if s < best {
            best = s;
        }
    }
    let mb = data.len() as f64 / (1024.0 * 1024.0);
    let rss = max_rss_kib();
    eprintln!(
        "bytes={} best_ns={:.0} MB/s={:.1} rss_kib={} threads={}",
        data.len(),
        best * 1e9,
        mb / best,
        rss,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
}

/// Peak resident set size in KiB (getrusage ru_maxrss on Linux).
fn max_rss_kib() -> i64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } == 0 {
        ru.ru_maxrss
    } else {
        -1
    }
}
