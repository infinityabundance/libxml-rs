//! §16.8.7 / §16.8.8 measurement harness — Rayon parallel blocking.
//!
//! Measures the candidate's `xmlReadMemory` end to end across the §16.8.7
//! size × thread matrix and reports the §16.8.8 scaling metrics. The resolved
//! `LIBXML_RS_PARALLEL` / `LIBXML_RS_THREADS` configuration is read once per
//! process (a `OnceLock`), so every cell runs in its own child process: the
//! parent re-execs itself with the cell's environment and collects the child's
//! `ns rss_kib` line.
//!
//! Shapes are generated deterministically in memory, so the parent and each
//! child build byte-identical documents without a corpus on disk. `bytes` is
//! the requested size class; the generator pads the final construct to land as
//! close as possible to it.
//!
//! Self-measured metrics (no oracle): throughput MB/s, speedup vs the 1-thread
//! optimized candidate, parallel efficiency, cycles/byte (using a caller-
//! supplied clock), RSS. Oracle-relative speedup (§16.8.8) is measured
//! separately in the oracle court with the same shapes (`tools/bench/harness.c`
//! `parse_comment` / `parse_cdata` ops) because provider isolation forbids
//! loading both providers into one address space.
//!
//! Usage:
//!   cargo run --release --example parbench -- [options]
//!
//! Options:
//!   --sizes   comma list, bytes (default 262144,1048576,4194304,16777216,67108864)
//!   --threads comma list (default 1,2,4,8,16)
//!   --shapes  comma list: one_comment,many_comment,one_cdata,many_cdata,
//!             long_text,dense (default all)
//!   --iters   timed iterations per cell (default 7)
//!   --ghz     clock for cycles/byte (default 4.8 — 9800X3D sustained all-core)
//!   --csv     emit only CSV rows (no commentary)
//!   --child SHAPE SIZE ITERS   internal mode

use std::io::Write;
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::time::Instant;

use libxml_rs::abi::exports_xml2::{xmlFreeDoc, xmlReadMemory};

// ── shape generation ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    OneComment,
    ManyComment,
    OneCdata,
    ManyCdata,
    LongText,
    Dense,
    /// Dense markup with a single 100 KiB comment: the "lone long run in a huge
    /// scan-light document" case that must NOT trigger a whole-input prepass.
    DenseComment,
}

impl Shape {
    fn parse(s: &str) -> Option<Shape> {
        Some(match s {
            "one_comment" => Shape::OneComment,
            "many_comment" => Shape::ManyComment,
            "one_cdata" => Shape::OneCdata,
            "many_cdata" => Shape::ManyCdata,
            "long_text" => Shape::LongText,
            "dense" => Shape::Dense,
            "dense_comment" => Shape::DenseComment,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Shape::OneComment => "one_comment",
            Shape::ManyComment => "many_comment",
            Shape::OneCdata => "one_cdata",
            Shape::ManyCdata => "many_cdata",
            Shape::LongText => "long_text",
            Shape::Dense => "dense",
            Shape::DenseComment => "dense_comment",
        }
    }
}

/// Generate a well-formed document of approximately `target` bytes.
fn generate(shape: Shape, target: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(target + 1024);
    out.extend_from_slice(b"<root>");
    match shape {
        Shape::OneComment => {
            out.extend_from_slice(b"<!--");
            let pad = target.saturating_sub(out.len() + b"--></root>".len());
            out.resize(out.len() + pad, b'c');
            out.extend_from_slice(b"--></root>");
        }
        Shape::OneCdata => {
            out.extend_from_slice(b"<![CDATA[");
            let pad = target.saturating_sub(out.len() + b"]]></root>".len());
            out.resize(out.len() + pad, b'x');
            out.extend_from_slice(b"]]></root>");
        }
        Shape::ManyComment | Shape::ManyCdata => {
            // 64 runs of near-equal length (the many-run amortisation case).
            let runs = 64usize;
            let open: &[u8] = if shape == Shape::ManyComment {
                b"<!--"
            } else {
                b"<![CDATA["
            };
            let close: &[u8] = if shape == Shape::ManyComment {
                b"-->"
            } else {
                b"]]>"
            };
            out.extend_from_slice(open);
            let body_total = target
                .saturating_sub(out.len() + b"</root>".len() + runs * (open.len() + close.len()));
            let per = body_total / runs;
            let filler = if shape == Shape::ManyComment {
                b'c'
            } else {
                b'x'
            };
            for _ in 0..runs.saturating_sub(1) {
                out.resize(out.len() + per, filler);
                out.extend_from_slice(close);
                out.extend_from_slice(open);
            }
            out.resize(out.len() + per, filler);
            out.extend_from_slice(close);
            out.extend_from_slice(b"</root>");
        }
        Shape::LongText => {
            let pad = target.saturating_sub(out.len() + b"</root>".len());
            out.resize(out.len() + pad, b'x');
            out.extend_from_slice(b"</root>");
        }
        Shape::Dense => {
            let mut i = 0u64;
            while out.len() + 64 < target {
                let _ = write!(out, "<item id=\"i{i}\">value{i}</item>");
                i += 1;
            }
            out.extend_from_slice(b"</root>");
        }
        Shape::DenseComment => {
            let half = target / 2;
            let mut inserted = false;
            let mut i = 0u64;
            while out.len() + 64 < target {
                if !inserted && out.len() >= half {
                    out.extend_from_slice(b"<!--");
                    out.resize(out.len() + 100 * 1024, b'c');
                    out.extend_from_slice(b"-->");
                    inserted = true;
                }
                let _ = write!(out, "<item id=\"i{i}\">value{i}</item>");
                i += 1;
            }
            out.extend_from_slice(b"</root>");
        }
    }
    out
}

// ── timing ──────────────────────────────────────────────────────────────────

fn max_rss_kib() -> i64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } == 0 {
        ru.ru_maxrss
    } else {
        -1
    }
}

fn time_cell(data: &[u8], iters: usize) -> f64 {
    // Warm up + sanity (also faults in the parse buffers).
    unsafe {
        let d = xmlReadMemory(
            data.as_ptr() as *const c_char,
            data.len() as c_int,
            ptr::null(),
            ptr::null(),
            0,
        );
        assert!(!d.is_null(), "warmup parse failed");
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
            assert!(!d.is_null(), "parse failed");
            xmlFreeDoc(d);
        }
        best = best.min(t.elapsed().as_secs_f64());
    }
    best
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Internal child mode: one cell, printed as `ns rss_kib`.
    if args.first().map(String::as_str) == Some("--child") {
        let shape = Shape::parse(&args[1]).expect("shape");
        let size: usize = args[2].parse().expect("size");
        let iters: usize = args[3].parse().expect("iters");
        let data = generate(shape, size);
        let best = time_cell(&data, iters);
        let rss = max_rss_kib();
        println!("{:.0} {}", best * 1e9, rss);
        return;
    }

    let mut sizes: Vec<usize> = vec![262144, 1048576, 4194304, 16777216, 67108864];
    let mut threads: Vec<usize> = vec![1, 2, 4, 8, 16];
    let mut shapes: Vec<Shape> = vec![
        Shape::OneComment,
        Shape::ManyComment,
        Shape::OneCdata,
        Shape::ManyCdata,
        Shape::LongText,
        Shape::Dense,
        Shape::DenseComment,
    ];
    let mut iters = 7usize;
    let mut ghz = 4.8f64;
    let mut reps = 1usize;
    let mut csv_only = false;

    let mut i = 0;
    while i < args.len() {
        let val = || args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--sizes" => {
                sizes = val().split(',').filter_map(|s| s.parse().ok()).collect();
                i += 2;
            }
            "--threads" => {
                threads = val().split(',').filter_map(|s| s.parse().ok()).collect();
                i += 2;
            }
            "--shapes" => {
                shapes = val().split(',').filter_map(Shape::parse).collect();
                i += 2;
            }
            "--iters" => {
                iters = val().parse().unwrap_or(7);
                i += 2;
            }
            "--reps" => {
                reps = val().parse().unwrap_or(1).max(1);
                i += 2;
            }
            "--ghz" => {
                ghz = val().parse().unwrap_or(4.8);
                i += 2;
            }
            "--csv" => {
                csv_only = true;
                i += 1;
            }
            other => {
                eprintln!("unknown option: {other}");
                std::process::exit(2);
            }
        }
    }

    let exe = std::env::current_exe().expect("current_exe");
    let mut out = std::io::stdout();

    println!("shape,bytes,mode,threads,best_ns,mb_per_s,speedup_vs_off1,efficiency,cycles_per_byte,rss_kib");

    for shape in shapes {
        for &size in &sizes {
            // Baseline: 1-thread, parallel off (the "one-thread optimized
            // candidate" of §16.8.8).
            let (base_ns, _) = run_child(&exe, shape, size, iters, "off", 1, reps);
            for &mode in &["off", "on", "auto"] {
                for &t in &threads {
                    // off is thread-count independent; keep one row.
                    if mode == "off" && t != 1 {
                        continue;
                    }
                    let (ns, rss) = run_child(&exe, shape, size, iters, mode, t, reps);
                    let mb = size as f64 / (1024.0 * 1024.0) / (ns / 1e9);
                    let speedup = base_ns / ns;
                    let eff = speedup / t as f64;
                    let cpb = ns * ghz / size as f64;
                    println!(
                        "{},{},{},{},{:.0},{:.1},{:.3},{:.3},{:.4},{}",
                        shape.name(),
                        size,
                        mode,
                        t,
                        ns,
                        mb,
                        speedup,
                        eff,
                        cpb,
                        rss
                    );
                }
            }
            let _ = out.flush();
        }
    }
    if !csv_only {
        eprintln!("(generated shapes in memory; columns defined in the header)");
    }
}

fn run_child(
    exe: &std::path::Path,
    shape: Shape,
    size: usize,
    iters: usize,
    mode: &str,
    threads: usize,
    reps: usize,
) -> (f64, i64) {
    let mut best = f64::MAX;
    let mut rss = -1i64;
    for _ in 0..reps.max(1) {
        let o = std::process::Command::new(exe)
            .arg("--child")
            .arg(shape.name())
            .arg(size.to_string())
            .arg(iters.to_string())
            .env("LIBXML_RS_PARALLEL", mode)
            .env("LIBXML_RS_THREADS", threads.to_string())
            .output()
            .expect("spawn child");
        assert!(
            o.status.success(),
            "child failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let s = String::from_utf8_lossy(&o.stdout);
        let mut it = s.split_whitespace();
        let ns: f64 = it.next().and_then(|x| x.parse().ok()).expect("child ns");
        if ns < best {
            best = ns;
        }
        if rss < 0 {
            rss = it.next().and_then(|x| x.parse().ok()).unwrap_or(-1);
        }
    }
    (best, rss)
}
