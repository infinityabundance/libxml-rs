//! Criterion micro-benchmarks for the libxml-rs drop-in C ABI.
//!
//! Each group measures a public C ABI entry point through the same path a
//! downstream consumer exercises (`xmlReadMemory`, `xmlXPathEvalExpression`,
//! `xmlDocDumpMemory`, `xsltApplyStylesheet`, `htmlReadMemory`). The Pareto
//! dimension is input size (bytes), reported as both latency and throughput.
//!
//! Run with:  cargo bench --bench benchmarks
//!
//! The oracle-vs-candidate Pareto matrix (speedup/latency vs upstream libxml2)
//! is produced separately by `tools/bench/pareto_matrix.py` in a minimal Docker
//! VM, which compiles a single C harness against both DSOs so the comparison
//! is apples-to-apples (same harness, same input, same allocator-visible path).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::os::raw::{c_char, c_int};
use std::ptr;

// ── C ABI surface under test (the drop-in exports) ──────────────────────────
use libxml_rs::abi::exports_html::htmlReadMemory;
use libxml_rs::abi::exports_xml2::{
    xmlDocDumpMemory, xmlFreeDoc, xmlReadMemory, xmlXPathEvalExpression, xmlXPathFreeContext,
    xmlXPathFreeObject, xmlXPathNewContext,
};
use libxml_rs::abi::types::xmlChar;
use libxml_rs::xslt::stylesheet::{xsltFreeStylesheet, xsltParseStylesheetDoc};
use libxml_rs::xslt::transform::xsltApplyStylesheet;

/// Build an element-heavy XML document of roughly `n` top-level items.
fn make_doc(items: usize) -> Vec<u8> {
    let mut s = String::with_capacity(items * 32 + 16);
    s.push_str("<root>");
    for i in 0..items {
        s.push_str(&format!("<item id=\"i{}\">value{}</item>", i, i));
    }
    s.push_str("</root>");
    s.into_bytes()
}

/// A minimal but non-trivial XSLT stylesheet (identity-ish with a computed
/// attribute), kept stable across benchmark runs.
const XSLT: &[u8] = br#"<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template match="/">
    <out><xsl:for-each select="root/item"><v><xsl:value-of select="."/></v></xsl:for-each></out>
  </xsl:template>
</xsl:stylesheet>"#;

/// HTML document with a table body.
fn make_html(rows: usize) -> Vec<u8> {
    let mut s = String::with_capacity(rows * 24 + 32);
    s.push_str("<html><body><table>");
    for i in 0..rows {
        s.push_str(&format!("<tr><td>cell{}</td></tr>", i));
    }
    s.push_str("</table></body></html>");
    s.into_bytes()
}

fn c_buf(data: &[u8]) -> (*const c_char, c_int) {
    (data.as_ptr() as *const c_char, data.len() as c_int)
}

// ── Parse ──────────────────────────────────────────────────────────────────
fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for &items in &[10usize, 100, 1000, 10_000] {
        let doc = make_doc(items);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(doc.len()), &doc, |b, doc| {
            b.iter(|| unsafe {
                let (buf, size) = c_buf(doc);
                let d = xmlReadMemory(buf, size, ptr::null(), ptr::null(), 0);
                black_box(d);
                if !d.is_null() {
                    xmlFreeDoc(d);
                }
            });
        });
    }
    group.finish();
}

// ── XPath ──────────────────────────────────────────────────────────────────
fn bench_xpath(c: &mut Criterion) {
    let mut group = c.benchmark_group("xpath");
    for &items in &[100usize, 1000, 10_000] {
        let doc = make_doc(items);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(doc.len()), &doc, |b, doc| {
            b.iter(|| unsafe {
                let (buf, size) = c_buf(doc);
                let d = xmlReadMemory(buf, size, ptr::null(), ptr::null(), 0);
                black_box(d);
                if !d.is_null() {
                    let ctx = xmlXPathNewContext(d);
                    let obj = xmlXPathEvalExpression(
                        b"//item[@id=\"i5\"]\0".as_ptr() as *const xmlChar,
                        ctx,
                    );
                    black_box(obj);
                    if !obj.is_null() {
                        xmlXPathFreeObject(obj);
                    }
                    xmlXPathFreeContext(ctx);
                    xmlFreeDoc(d);
                }
            });
        });
    }
    group.finish();
}

// ── Serialize ──────────────────────────────────────────────────────────────
fn bench_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialize");
    for &items in &[100usize, 1000, 10_000] {
        let doc = make_doc(items);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(doc.len()), &doc, |b, doc| {
            b.iter(|| unsafe {
                let (buf, size) = c_buf(doc);
                let d = xmlReadMemory(buf, size, ptr::null(), ptr::null(), 0);
                black_box(d);
                if !d.is_null() {
                    let mut mem: *mut xmlChar = ptr::null_mut();
                    let mut out_size: c_int = 0;
                    xmlDocDumpMemory(d, &mut mem, &mut out_size);
                    black_box((mem, out_size));
                    if !mem.is_null() {
                        libxml_rs::abi::allocator::xmlFreeImpl(mem as *mut core::ffi::c_void);
                    }
                    xmlFreeDoc(d);
                }
            });
        });
    }
    group.finish();
}

// ── XSLT ───────────────────────────────────────────────────────────────────
fn bench_xslt(c: &mut Criterion) {
    let mut group = c.benchmark_group("xslt");
    for &items in &[10usize, 100, 1000] {
        let src = make_doc(items);
        group.throughput(Throughput::Bytes(src.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(src.len()), &src, |b, src| {
            // Compile the stylesheet once per size bucket (outside the timed
            // loop); the transform itself is what is measured.
            let style = unsafe {
                let (xb, xs) = c_buf(XSLT);
                let sd = xmlReadMemory(xb, xs, ptr::null(), ptr::null(), 0);
                let st = xsltParseStylesheetDoc(sd);
                st
            };
            b.iter(|| unsafe {
                let (buf, size) = c_buf(src);
                let d = xmlReadMemory(buf, size, ptr::null(), ptr::null(), 0);
                black_box(d);
                if !d.is_null() {
                    let out = xsltApplyStylesheet(style, d, ptr::null_mut());
                    black_box(out);
                    if !out.is_null() {
                        xmlFreeDoc(out);
                    }
                    xmlFreeDoc(d);
                }
            });
            unsafe {
                if !style.is_null() {
                    xsltFreeStylesheet(style);
                }
            }
        });
    }
    group.finish();
}

// ── HTML parse ─────────────────────────────────────────────────────────────
fn bench_html(c: &mut Criterion) {
    let mut group = c.benchmark_group("html");
    for &rows in &[10usize, 100, 1000, 10_000] {
        let doc = make_html(rows);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(doc.len()), &doc, |b, doc| {
            b.iter(|| unsafe {
                let (buf, size) = c_buf(doc);
                let d = htmlReadMemory(buf, size, ptr::null(), ptr::null(), 0);
                black_box(d);
                if !d.is_null() {
                    xmlFreeDoc(d);
                }
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_xpath,
    bench_serialize,
    bench_xslt,
    bench_html
);
criterion_main!(benches);
