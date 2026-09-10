//! §16.7.8 slice 1 — ORACLE-SHADOW court for the persistent push driver.
//!
//! The driver's own court (`pushdrive::tests`) compares it to the candidate's
//! RECURSIVE parser and to internal invariants. This court compares it to
//! **libxml2 2.15.3 itself**, per call.
//!
//! # How the two sides are produced
//!
//! ```text
//! oracle : courts/suites/phase16/pushdiff-probe.c, run by
//!          pushdrive-shadow-run.sh against the system libxml2, over
//!          courts/suites/phase16/shadow-corpus and a fixed plan list;
//!          the raw per-call traces are committed under
//!          courts/receipts/phase-16/raw/pushdrive-shadow/.
//! driver : this module, driving `pushdrive` over the same documents and the
//!          same plan shapes, emitting the SAME record grammar.
//! ```
//!
//! Both sides emit the probe's grammar, so the comparison is a line-sequence
//! comparison:
//!
//! ```text
//! == <doc> bytes=<n> mode=<mode>
//! > CTOR len=<n>
//! < CTOR ok=1 err=<e> wf=<w> in=<i> p=<p> l=<l> col=<c> i=<inNr> n=<nameNr>
//! > CALL <n> len=<n>          (or  > CALL <n> ZERO  for a zero-length call)
//! <event lines, in dispatch order>
//! < CALL <n> rc=<rc> err=<e> wf=<w> in=<i> p=<p> l=<l> col=<c> i=<i> n=<n>
//! > FINAL  / < FINAL 0 ...
//! > REFEED / < REFEED 0 ...
//! ```
//!
//! # What is compared, and what is deliberately not
//!
//! Compared per call: the return code, `errNo`, `wellFormed`, `instate`,
//! `cur - base`, line, column, `inputNr`, `nameNr`, and the ordered SAX events
//! **with payload and segmentation** (`startDocument`/`endDocument`,
//! start/end element with namespace + attribute detail, `characters` (length
//! AND bytes), `comment`, `cdata`, `pi`, `reference`, `ignorableWhitespace`).
//!
//! NOT compared:
//!
//! - **Structured diagnostic records** (the probe's `error dom=.. code=..`
//!   lines). Error *occurrence and code* are already compared through `rc` and
//!   `errNo`; the message text/positions are the existing courts' surface
//!   (`pushdiff-*`, ERROR-001).
//! - **`p` is the absolute offset into the current materialization**, not a
//!   pointer difference. That is the same quantity upstream reports as
//!   `cur - base` because the candidate never rebases its buffer
//!   (`xmlParserShrink` is a no-op here); the driver-side `p` comes from
//!   `InputBuffer::pos().2` directly rather than from `ctxt->input`, which the
//!   court-only driver does not publish.
//! - The driver's own machine diagnostics (materialized/consumed/scan-work)
//!   are printed SEPARATELY by the report and never enter the compared trace.
//!
//! # Known-red cells
//!
//! The caller-selected green set is asserted; everything else is REPORTED as a
//! divergence so the court is a visible baseline rather than a silently
//! partial gate. `shadow-utf16trunc.xml` is expected red on every plan: it is
//! the encoder-flush cell (`xmlParserCheckEOF`'s `source_truncated` half, which
//! `pushdrive::check_eof` documents as unimplemented).

#![allow(dead_code)]

use crate::abi::structs::{_xmlParserCtxt, _xmlSAXHandler};
use crate::abi::types::{xmlChar, XML_SAX2_MAGIC};
use crate::xml::parser::helpers;
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::push::PushMachine;
use crate::xml::parser::state::XmlParser;
use std::cell::RefCell;
use std::os::raw::{c_int, c_void};

pub(crate) const CORPUS_DIR: &str = "courts/suites/phase16/shadow-corpus";
pub(crate) const FIXTURE_DIR: &str = "courts/receipts/phase-16/raw/pushdrive-shadow";

thread_local! {
    /// Event lines emitted by the recorder while a driver pass runs.
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn emit(line: String) {
    EVENTS.with(|e| e.borrow_mut().push(line));
}

fn drain_events() -> Vec<String> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

// ── probe-compatible escaping ───────────────────────────────────────────────

/// Mirror of the probe's `esc_bytes`: `\\`, `\n`, `\r`, `\t`, printable ASCII
/// verbatim, everything else `\xNN` (lowercase, two digits).
fn esc_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

unsafe fn c_len(p: *const xmlChar) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0usize;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    n
}

/// Mirror of the probe's `esc_str` (NUL-terminated; NULL is empty).
unsafe fn esc_str(p: *const xmlChar) -> String {
    if p.is_null() {
        return String::new();
    }
    let len = unsafe { c_len(p) };
    unsafe { esc_bytes(std::slice::from_raw_parts(p, len)) }
}

// ── the recording SAX handler (the probe's recorder, in Rust) ───────────────

unsafe extern "C" fn on_start_document(_ctx: *mut c_void) {
    emit("startDocument".to_string());
}

unsafe extern "C" fn on_end_document(_ctx: *mut c_void) {
    emit("endDocument".to_string());
}

unsafe extern "C" fn on_start_ns(
    _ctx: *mut c_void,
    localname: *const xmlChar,
    prefix: *const xmlChar,
    uri: *const xmlChar,
    nb_namespaces: c_int,
    namespaces: *mut *const xmlChar,
    nb_attributes: c_int,
    nb_defaulted: c_int,
    attributes: *mut *const xmlChar,
) {
    let mut line = String::from("startElementNs local=[");
    line.push_str(&unsafe { esc_str(localname) });
    line.push_str("] prefix=");
    line.push_str(&unsafe { esc_str(prefix) });
    line.push_str(" uri=");
    line.push_str(&unsafe { esc_str(uri) });
    line.push_str(&format!(
        " ns={nb_namespaces} att={nb_attributes} def={nb_defaulted}"
    ));
    for i in (0..nb_namespaces as usize * 2).step_by(2) {
        let p = unsafe { *namespaces.add(i) };
        let u = unsafe { *namespaces.add(i + 1) };
        line.push_str(" {");
        line.push_str(&unsafe { esc_str(p) });
        line.push('=');
        line.push_str(&unsafe { esc_str(u) });
        line.push('}');
    }
    for i in (0..nb_attributes as usize * 5).step_by(5) {
        let l = unsafe { *attributes.add(i) };
        let p = unsafe { *attributes.add(i + 1) };
        let u = unsafe { *attributes.add(i + 2) };
        let v = unsafe { *attributes.add(i + 3) };
        let vend = unsafe { *attributes.add(i + 4) };
        // The (value, valueEnd) length convention: a non-NULL valueEnd is the
        // authoritative end pointer; otherwise the value is NUL-terminated.
        let vlen = if v.is_null() {
            0
        } else if !vend.is_null() {
            unsafe { vend.offset_from(v) as usize }
        } else {
            unsafe { c_len(v) }
        };
        line.push_str(" [a local=");
        line.push_str(&unsafe { esc_str(l) });
        line.push_str(" prefix=");
        line.push_str(&unsafe { esc_str(p) });
        line.push_str(" uri=");
        line.push_str(&unsafe { esc_str(u) });
        line.push_str(" value=");
        line.push_str(&unsafe { esc_bytes(std::slice::from_raw_parts(v, vlen)) });
        line.push_str(&format!(" end={}]", if vend.is_null() { 0 } else { 1 }));
    }
    emit(line);
}

unsafe extern "C" fn on_end_ns(
    _ctx: *mut c_void,
    localname: *const xmlChar,
    prefix: *const xmlChar,
    uri: *const xmlChar,
) {
    let mut line = String::from("endElementNs local=[");
    line.push_str(&unsafe { esc_str(localname) });
    line.push_str("] prefix=");
    line.push_str(&unsafe { esc_str(prefix) });
    line.push_str(" uri=");
    line.push_str(&unsafe { esc_str(uri) });
    emit(line);
}

unsafe extern "C" fn on_characters(_ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    let n = len.max(0) as usize;
    let bytes = if ch.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(ch, n) }
    };
    emit(format!("characters len={n} [{}]", esc_bytes(bytes)));
}

unsafe extern "C" fn on_ignorable(_ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    let n = len.max(0) as usize;
    let bytes = if ch.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(ch, n) }
    };
    emit(format!(
        "ignorableWhitespace len={n} [{}]",
        esc_bytes(bytes)
    ));
}

unsafe extern "C" fn on_comment(_ctx: *mut c_void, value: *const xmlChar) {
    emit(format!("comment [{}]", unsafe { esc_str(value) }));
}

unsafe extern "C" fn on_cdata(_ctx: *mut c_void, value: *const xmlChar, len: c_int) {
    let n = len.max(0) as usize;
    let bytes = if value.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(value, n) }
    };
    emit(format!("cdata len={n} [{}]", esc_bytes(bytes)));
}

unsafe extern "C" fn on_pi(_ctx: *mut c_void, target: *const xmlChar, data: *const xmlChar) {
    emit(format!(
        "pi target={} data={}",
        unsafe { esc_str(target) },
        unsafe { esc_str(data) }
    ));
}

unsafe extern "C" fn on_reference(_ctx: *mut c_void, name: *const xmlChar) {
    emit(format!("reference [{}]", unsafe { esc_str(name) }));
}

/// Replace the context's SAX handler with the recorder, in place (the handler
/// allocation is owned by the context and freed by `free_parser_ctxt`).
unsafe fn install_recorder(ctxt: *mut _xmlParserCtxt) {
    let h: *mut _xmlSAXHandler = unsafe { (*ctxt).sax };
    assert!(!h.is_null(), "context has no SAX handler");
    unsafe {
        std::ptr::write_bytes(h as *mut u8, 0, std::mem::size_of::<_xmlSAXHandler>());
        (*h).startDocument = Some(on_start_document);
        (*h).endDocument = Some(on_end_document);
        (*h).startElementNs = Some(on_start_ns);
        (*h).endElementNs = Some(on_end_ns);
        (*h).characters = Some(on_characters);
        (*h).ignorableWhitespace = Some(on_ignorable);
        (*h).comment = Some(on_comment);
        (*h).cdataBlock = Some(on_cdata);
        (*h).processingInstruction = Some(on_pi);
        (*h).reference = Some(on_reference);
        // SAX2 dispatch requires the magic and a startElementNs slot.
        (*h).initialized = XML_SAX2_MAGIC as u32;
    }
}

// ── plans (the probe's `[C]bN[zK]` subset, deterministic) ───────────────────

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowPlan {
    pub fixed: usize,
    pub ctor: bool,
    pub zero_after: usize,
}

impl ShadowPlan {
    pub fn parse(mode: &str) -> Option<Self> {
        let mut s = mode;
        let mut ctor = false;
        if let Some(rest) = s.strip_prefix('C') {
            ctor = true;
            s = rest;
        }
        let (base, zeros) = match s.find('z') {
            Some(i) => (&s[..i], s[i + 1..].parse::<usize>().ok()?),
            None => (s, 0),
        };
        let fixed = base.strip_prefix('b')?.parse::<usize>().ok()?;
        Some(ShadowPlan {
            fixed,
            ctor,
            zero_after: zeros,
        })
    }

    /// `bN` with N >= the document length is upstream's "whole document in one
    /// non-final chunk" shape.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctor {
            s.push('C');
        }
        s.push_str(&format!("b{}", self.fixed));
        if self.zero_after > 0 {
            s.push_str(&format!("z{}", self.zero_after));
        }
        s
    }
}

// ── the driver side ─────────────────────────────────────────────────────────

struct CtxtGuard(*mut _xmlParserCtxt);
impl Drop for CtxtGuard {
    fn drop(&mut self) {
        unsafe { helpers::free_parser_ctxt(self.0) };
    }
}

/// The per-call tail line, in the probe's exact shape.
///
/// `rc` is not a driver return value: `xmlParseChunk` returns `errNo` when the
/// document is not well-formed and 0 otherwise, so the same expression is
/// evaluated from the context state the driver left behind.
unsafe fn tail_line(
    lbl: &str,
    idx: usize,
    ctxt: *mut _xmlParserCtxt,
    parser: &XmlParser,
    ctor: bool,
) -> String {
    let (line, col, pos) = parser.base_input().pos();
    let err = unsafe { (*ctxt).errNo };
    let wf = unsafe { (*ctxt).wellFormed };
    let instate = unsafe { (*ctxt).instate };
    let name_nr = unsafe { (*ctxt).nameNr };
    let head = if ctor {
        format!("< {lbl} ok=1 err={err} wf={wf} in={instate}")
    } else {
        let rc = if wf == 0 { err } else { 0 };
        format!("< {lbl} {idx} rc={rc} err={err} wf={wf} in={instate}")
    };
    format!("{head} p={pos} l={line} col={col} i=1 n={name_nr}")
}

/// Drive one document through one plan and emit the probe-grammar trace.
pub(crate) fn run_driver(doc: &[u8], plan: &ShadowPlan, name: &str) -> Vec<String> {
    let mode = plan.label();
    let mut out = vec![format!("== {name} bytes={} mode={mode}", doc.len())];
    unsafe {
        let guard = CtxtGuard(helpers::create_parser_ctxt());
        let ctxt = guard.0;
        assert!(!ctxt.is_null());
        install_recorder(ctxt);

        // `p` borrows the raw context pointer, so it must be dropped before
        // the guard; declaring it here keeps that ordering explicit.
        let mut parser: XmlParser;
        let mut machine = PushMachine::new();
        let mut off = 0usize;
        let mut call = 0usize;

        if plan.ctor && !doc.is_empty() {
            let n = plan.fixed.min(doc.len());
            out.push(format!("> CTOR len={n}"));
            drain_events();
            let buf = InputBuffer::for_push(&doc[..n], None);
            parser = XmlParser::new_with_flags(InputStack::new(buf), ctxt, false, false);
            let _ = parser.drive_persistent(&mut machine, false);
            out.append(&mut drain_events());
            out.push(tail_line("CTOR", 0, ctxt, &parser, true));
            off = n;
        } else {
            let buf = InputBuffer::for_push(&[], None);
            parser = XmlParser::new_with_flags(InputStack::new(buf), ctxt, false, false);
        }

        while off < doc.len() {
            let n = plan.fixed.min(doc.len() - off);
            out.push(format!("> CALL {call} len={n}"));
            drain_events();
            let _ = parser.push_persistent(&mut machine, &doc[off..off + n], false);
            out.append(&mut drain_events());
            out.push(tail_line("CALL", call, ctxt, &parser, false));
            off += n;
            // The probe increments the call index BEFORE injecting the
            // zero-length calls, so their label is the NEXT index.
            call += 1;
            for _ in 0..plan.zero_after {
                out.push(format!("> CALL {call} ZERO"));
                drain_events();
                let _ = parser.push_persistent(&mut machine, &[], false);
                out.append(&mut drain_events());
                out.push(tail_line("CALL", call, ctxt, &parser, false));
            }
        }

        out.push("> FINAL".to_string());
        drain_events();
        let _ = parser.push_persistent(&mut machine, &[], true);
        out.append(&mut drain_events());
        out.push(tail_line("FINAL", 0, ctxt, &parser, false));

        out.push("> REFEED".to_string());
        drain_events();
        let _ = parser.push_persistent(&mut machine, doc, true);
        out.append(&mut drain_events());
        out.push(tail_line("REFEED", 0, ctxt, &parser, false));
    }
    out
}

// ── the oracle side ─────────────────────────────────────────────────────────

/// The probe prints the absolute path it was handed; normalize it to the
/// basename so the two sides are comparable regardless of where each ran.
pub(crate) fn normalize_oracle(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| match l.strip_prefix("== ") {
            Some(rest) => {
                let (path, tail) = match rest.split_once(' ') {
                    Some((p, t)) => (p, t),
                    None => (rest, ""),
                };
                let base = path.rsplit('/').next().unwrap_or(path);
                format!("== {base} {tail}")
            }
            None => l.to_string(),
        })
        .collect()
}

// ── the report ──────────────────────────────────────────────────────────────

pub(crate) struct CellReport {
    pub doc: String,
    pub mode: String,
    pub status: CellStatus,
    pub first_diff: Option<(usize, String, String)>,
    /// First `p` (physical cursor) disagreement, as an observation.
    pub cursor_diff: Option<(usize, String, String)>,
    pub oracle_lines: usize,
    pub driver_lines: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CellStatus {
    Match,
    Diverge,
}

// ── the compared projection ────────────────────────────────────────────

/// The line sequence the court asserts on.
///
/// Two exclusions, both deliberate and documented in the module docs:
///
/// 1. Structured diagnostic records (the probe's `error dom=.. code=..`) —
///    error occurrence and code are already compared through the call record's
///    `rc`/`err`; the message text is the existing courts' surface.
/// 2. The `p=` field. `p` is `cur - base` in the ORACLE, and upstream REBASES
///    that buffer above 4096 bytes (`xmlParserShrink`, and again through
///    `xmlBufUpdateInput` in `xmlParserCheckEOF`'s encoder flush). The
///    court-only driver does not publish `ctxt->input` at all yet, so `p` is
///    not a driver surface: it is measured and REPORTED (see
///    [`cursor_divergence`]) but not asserted until the driver publishes the
///    input and models the rebasing.
fn project(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| !l.starts_with("error "))
        .map(|l| match l.find(" p=") {
            Some(i) => {
                let rest = &l[i + 3..];
                let after = rest.find(' ').map(|j| &rest[j..]).unwrap_or("");
                format!("{}{}", &l[..i], after)
            }
            None => l.clone(),
        })
        .collect()
}

fn p_of(line: &str) -> Option<String> {
    let i = line.find(" p=")? + 3;
    let rest = &line[i..];
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// The first `p` value that differs, as an OBSERVATION (never an assertion).
fn cursor_divergence(oracle: &[String], driver: &[String]) -> Option<(usize, String, String)> {
    // Aligned on the same projection the assertion uses (diagnostic records
    // dropped), so `p` is read from genuinely corresponding lines.
    let no_errors = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter(|l| !l.starts_with("error "))
            .cloned()
            .collect()
    };
    let o = no_errors(oracle);
    let d = no_errors(driver);
    for (i, (ol, dl)) in o.iter().zip(d.iter()).enumerate() {
        if p_of(ol) != p_of(dl) {
            return Some((
                i,
                p_of(ol).unwrap_or_default(),
                p_of(dl).unwrap_or_default(),
            ));
        }
    }
    None
}

pub(crate) fn compare(
    oracle: &[String],
    driver: &[String],
) -> (CellStatus, Option<(usize, String, String)>) {
    let oracle = project(oracle);
    let driver = project(driver);
    let n = oracle.len().min(driver.len());
    for i in 0..n {
        if oracle[i] != driver[i] {
            return (
                CellStatus::Diverge,
                Some((i, oracle[i].clone(), driver[i].clone())),
            );
        }
    }
    if oracle.len() != driver.len() {
        let i = n;
        let o = oracle
            .get(i)
            .cloned()
            .unwrap_or_else(|| "<eof>".to_string());
        let d = driver
            .get(i)
            .cloned()
            .unwrap_or_else(|| "<eof>".to_string());
        return (CellStatus::Diverge, Some((i, o, d)));
    }
    (CellStatus::Match, None)
}

/// Run every committed oracle fixture and compare. Returns one report per cell
/// in deterministic order.
pub(crate) fn run_all() -> Vec<CellReport> {
    let mut fixtures: Vec<_> = std::fs::read_dir(FIXTURE_DIR)
        .expect("shadow fixtures missing — run pushdrive-shadow-run.sh")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("oracle-"))
        .collect();
    fixtures.sort();

    let mut reports = Vec::new();
    for fixture in fixtures {
        let Some(rest) = fixture.strip_prefix("oracle-") else {
            continue;
        };
        let Some((doc_name, mode)) = rest.rsplit_once("__") else {
            continue;
        };
        let Some(plan) = ShadowPlan::parse(mode) else {
            continue;
        };
        let doc = std::fs::read(format!("{CORPUS_DIR}/{doc_name}"))
            .unwrap_or_else(|_| panic!("shadow corpus document missing: {doc_name}"));
        let oracle_text = std::fs::read_to_string(format!("{FIXTURE_DIR}/{fixture}"))
            .expect("oracle fixture unreadable");
        let oracle = normalize_oracle(&oracle_text);
        let driver = run_driver(&doc, &plan, doc_name);
        let (status, first_diff) = compare(&oracle, &driver);
        let cursor_diff = cursor_divergence(&oracle, &driver);
        reports.push(CellReport {
            doc: doc_name.to_string(),
            mode: mode.to_string(),
            status,
            first_diff,
            cursor_diff,
            oracle_lines: oracle.len(),
            driver_lines: driver.len(),
        });
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The encoder-flush cell (`xmlParserCheckEOF`'s `source_truncated` half,
    /// documented as unimplemented in `pushdrive::check_eof`). It is the one
    /// document expected to diverge on EVERY plan.
    const KNOWN_RED_DOC: &str = "shadow-utf16trunc.xml";

    /// Report the whole shadow court and assert that every cell OUTSIDE the
    /// documented known-red document matches the oracle.
    ///
    /// Ignored by default only because it needs the committed oracle fixtures
    /// (generated by `pushdrive-shadow-run.sh`); run it with
    /// `cargo test --lib pushshadow -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the committed oracle fixtures; run explicitly"]
    fn shadow_court_report() {
        let reports = run_all();
        assert!(!reports.is_empty(), "no shadow cells found");

        let mut matched = 0usize;
        let mut diverged = Vec::new();
        for r in &reports {
            match r.status {
                CellStatus::Match => matched += 1,
                CellStatus::Diverge => diverged.push(r),
            }
        }
        println!(
            "shadow: {} cells, {} match, {} diverge",
            reports.len(),
            matched,
            diverged.len()
        );
        for r in &diverged {
            println!(
                "  DIVERGE {} mode={} (oracle {} lines, driver {} lines)",
                r.doc, r.mode, r.oracle_lines, r.driver_lines
            );
            if let Some((i, o, d)) = &r.first_diff {
                println!("    first diff at line {i}");
                println!("      oracle: {o}");
                println!("      driver: {d}");
            }
        }

        // `p` (the physical `cur - base`) is observed, not asserted: upstream
        // rebases that buffer above 4096 bytes and the court-only driver does
        // not publish `ctxt->input` yet.
        let cursor_cells = reports.iter().filter(|r| r.cursor_diff.is_some()).count();
        println!("  cursor (p) disagreements observed in {cursor_cells} cells:");
        for r in reports.iter() {
            if let Some((i, po, pd)) = &r.cursor_diff {
                println!(
                    "    {} mode={} line {} oracle p={} driver p={}",
                    r.doc, r.mode, i, po, pd
                );
            }
        }

        // The gate: only the documented known-red document may diverge.
        let unexpected: Vec<_> = diverged
            .iter()
            .filter(|r| r.doc != KNOWN_RED_DOC)
            .map(|r| format!("{} [{}]", r.doc, r.mode))
            .collect();
        assert!(
            unexpected.is_empty(),
            "shadow court diverged outside the known-red document: {unexpected:#?}"
        );

        // And the known-red document must actually exercise the court (a
        // silently-passing fixture would hide the remainder).
        assert!(
            diverged.iter().any(|r| r.doc == KNOWN_RED_DOC),
            "the encoder-flush cell ({KNOWN_RED_DOC}) was expected to diverge but did not"
        );
    }
}
