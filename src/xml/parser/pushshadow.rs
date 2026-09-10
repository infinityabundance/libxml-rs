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
//! line, column, `nameNr`, the ordered SAX events **with payload and
//! segmentation** (`startDocument`/`endDocument`, start/end element with
//! namespace + attribute detail, `characters` (length AND bytes), `comment`,
//! `cdata`, `pi`, `reference`, `ignorableWhitespace`), and the ordered
//! structured DIAGNOSTICS in canonical form (`dom`/`code`/`level`/`line`/
//! `int1`/`int2`) — canonicalised rather than dropped, so `error 81` before
//! `endDocument` is distinguishable from `endDocument` before `error 81`.
//!
//! The PHYSICAL WINDOW is compared too, in the probe's `-W` fields:
//!
//! - `p` (`cur - base`), `c` (`input->consumed`), `abs` (`c + p`),
//!   `i` (`inputNr`) — read from the published ABI window;
//! - `pos0`/`line0`/`col0`/`byte0` — the exported `xmlCtxtGetInputPosition`;
//! - `win0`/`wsize0`/`woff0`/`wbytes0` — the exported `xmlCtxtGetInputWindow`,
//!   INCLUDING the window bytes, which prove the materialized buffer around
//!   the cursor agrees (the raw pointer fields cannot show that). Calling the
//!   exports rather than re-reading the struct also proves `inputTab[0]` is
//!   published, because they index `inputTab`, not `ctxt->input`.
//!
//! `! cb=<name> ...` lines record the same window at SAX/error CALLBACK ENTRY,
//! so the court proves the C-visible pointers are refreshed before observable
//! dispatch, not merely at the call tail.
//!
//! For the two ENCODER documents the full diagnostic record (including the
//! message payload) is asserted as well, because those records are stable and
//! are the contract under test.
//!
//! NOT compared:
//!
//! - **Diagnostic message windows** (`file`, `str1..3`, `msg`) outside the
//!   encoder pair. Those are the existing error courts' surface, and
//!   declarations/DTDs will make them complicated.
//! - The driver's own machine diagnostics (materialized/consumed/scan-work)
//!   are printed SEPARATELY by the report and never enter the compared trace.
//!
//! # Coverage
//!
//! Every corpus document x its expected plan set must be present, and the
//! total must be the exact expected cell count: the matrix is asserted in
//! Rust with constants that are deliberately INDEPENDENT of the shell
//! runner's mode strings, so deleting a mode from the runner and regenerating
//! the fixtures cannot quietly redefine "complete".
//!
//! # Known-red cells
//!
//! NONE. The encoder-flush cell (`shadow-utf16trunc.xml`) and the
//! definite-invalid cell (`shadow-utf16invalid.xml`) both pass.

#![allow(dead_code)]

use crate::abi::structs::{_xmlError, _xmlParserCtxt, _xmlSAXHandler};
use crate::abi::types::{xmlChar, XML_SAX2_MAGIC};
use crate::xml::parser::helpers;
use crate::xml::parser::input::{InputBuffer, InputStack};
use crate::xml::parser::push::PushMachine;
use crate::xml::parser::state::XmlParser;
use std::cell::RefCell;
use std::os::raw::{c_int, c_ulong, c_void};

pub(crate) const CORPUS_DIR: &str = "courts/suites/phase16/shadow-corpus";
pub(crate) const FIXTURE_DIR: &str = "courts/receipts/phase-16/raw/pushdrive-shadow";

/// The exact fixture matrix, hard-coded HERE and deliberately independent of
/// the shell runner's mode strings: if a mode is deleted from the launcher and
/// the fixtures are regenerated, the court must refuse to quietly redefine
/// "complete" rather than silently narrowing its own evidence.
const SMALL_MODES: &[&str] = &[
    "b1", "b2", "b3", "b5", "b257", "Cb1", "b1z2", "b1i", "Cb1i", "r9-2",
];
const LONG_MODES: &[&str] = &["b1024", "b4096", "b1024i", "r17-512"];
/// The physical-window threshold archaeology documents (`shadow-win-*`).
const WINDOW_MODES: &[&str] = &["b1024", "b4096", "b1024i"];
/// Documents at or below this many bytes use [`SMALL_MODES`]. Duplicates the
/// launcher's threshold on purpose (see [`SMALL_MODES`]).
const SMALL_DOC_MAX: usize = 200;
/// 5 small x 10 + 2 encoder x 10 + 2 long x 4 + 8 window x 3.
const SHADOW_CELL_TOTAL: usize = 102;

/// Documents whose FULL diagnostic records (message payload included) are
/// asserted, not just their canonical form.
const ENCODER_DOCS: &[&str] = &["shadow-utf16trunc.xml", "shadow-utf16invalid.xml"];

thread_local! {
    /// Event lines emitted by the recorder while a driver pass runs.
    static EVENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The live context, so the recorder callbacks can read the published ABI
    /// window at callback ENTRY. The SAX user-data slot is NULL (the probe's
    /// handler table leaves it unset), so the context pointer is not otherwise
    /// reachable from inside a callback.
    static CTXT: std::cell::Cell<*mut _xmlParserCtxt> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

fn emit(line: String) {
    EVENTS.with(|e| e.borrow_mut().push(line));
}

fn drain_events() -> Vec<String> {
    EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

/// The window as seen from INSIDE a callback, in the probe's `-W` `cbmark`
/// grammar. Emitted before every event so the court proves the C-visible
/// pointers are refreshed before observable dispatch, not merely at the call
/// tail.
fn cbmark(what: &str) {
    let ctxt = CTXT.with(|c| c.get());
    let input = if ctxt.is_null() {
        std::ptr::null_mut()
    } else {
        unsafe { (*ctxt).input }
    };
    if input.is_null() {
        emit(format!("! cb={what} p=-1 c=-1 abs=-1"));
        return;
    }
    let p = unsafe { (*input).cur.offset_from((*input).base) };
    let cons = unsafe { (*input).consumed };
    emit(format!(
        "! cb={what} p={p} c={cons} abs={}",
        cons as i64 + p as i64
    ));
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
    cbmark("startDocument");
    emit("startDocument".to_string());
}

unsafe extern "C" fn on_end_document(_ctx: *mut c_void) {
    cbmark("endDocument");
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
    cbmark("startElementNs");
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
    cbmark("endElementNs");
    let mut line = String::from("endElementNs local=[");
    line.push_str(&unsafe { esc_str(localname) });
    line.push_str("] prefix=");
    line.push_str(&unsafe { esc_str(prefix) });
    line.push_str(" uri=");
    line.push_str(&unsafe { esc_str(uri) });
    emit(line);
}

unsafe extern "C" fn on_characters(_ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    cbmark("characters");
    let n = len.max(0) as usize;
    let bytes = if ch.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(ch, n) }
    };
    emit(format!("characters len={n} [{}]", esc_bytes(bytes)));
}

unsafe extern "C" fn on_ignorable(_ctx: *mut c_void, ch: *const xmlChar, len: c_int) {
    cbmark("ignorableWhitespace");
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
    cbmark("comment");
    emit(format!("comment [{}]", unsafe { esc_str(value) }));
}

unsafe extern "C" fn on_cdata(_ctx: *mut c_void, value: *const xmlChar, len: c_int) {
    cbmark("cdataBlock");
    let n = len.max(0) as usize;
    let bytes = if value.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(value, n) }
    };
    emit(format!("cdata len={n} [{}]", esc_bytes(bytes)));
}

unsafe extern "C" fn on_pi(_ctx: *mut c_void, target: *const xmlChar, data: *const xmlChar) {
    cbmark("processingInstruction");
    emit(format!(
        "pi target={} data={}",
        unsafe { esc_str(target) },
        unsafe { esc_str(data) }
    ));
}

unsafe extern "C" fn on_reference(_ctx: *mut c_void, name: *const xmlChar) {
    cbmark("reference");
    emit(format!("reference [{}]", unsafe { esc_str(name) }));
}

/// Structured-error recorder, byte-compatible with the probe's `rec_err`.
///
/// Installing this puts diagnostics into the SAME ordered stream as the SAX
/// events, which is what makes error OCCURRENCE AND ORDERING executable
/// evidence ("error 81, then endDocument", not merely "the final errNo was
/// 81").
unsafe extern "C" fn on_error(_ctx: *mut c_void, error: *const _xmlError) {
    cbmark("error");
    if error.is_null() {
        emit("error(NULL)".to_string());
        return;
    }
    let e = unsafe { &*error };
    let mut line = format!(
        "error dom={} code={} level={} file=",
        e.domain, e.code, e.level
    );
    line.push_str(&unsafe { esc_str(e.file as *const xmlChar) });
    line.push_str(&format!(
        " line={} i1={} i2={} str1=",
        e.line, e.int1, e.int2
    ));
    line.push_str(&unsafe { esc_str(e.str1 as *const xmlChar) });
    line.push_str(" str2=");
    line.push_str(&unsafe { esc_str(e.str2 as *const xmlChar) });
    line.push_str(" str3=");
    line.push_str(&unsafe { esc_str(e.str3 as *const xmlChar) });
    line.push_str(" msg=");
    line.push_str(&unsafe { esc_str(e.message as *const xmlChar) });
    emit(line);
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

#[derive(Clone, Debug)]
pub(crate) struct ShadowPlan {
    /// The original mode string (reproduced verbatim in the trace header).
    pub mode: String,
    pub random: bool,
    pub fixed: usize,
    pub seed: u64,
    pub span: u32,
    pub ctor: bool,
    pub zero_after: usize,
    /// The LAST real chunk carries `terminate = 1` and no separate empty
    /// terminating call follows. Feeding the bytes and terminating in ONE
    /// call is a distinct input shape: the decoder flush sees the bytes and
    /// the end-of-stream together.
    pub inline_final: bool,
}

impl ShadowPlan {
    pub fn parse(mode: &str) -> Option<Self> {
        let mut s = mode;
        let mut ctor = false;
        if let Some(rest) = s.strip_prefix('C') {
            ctor = true;
            s = rest;
        }
        let (base, mut rest) = match s.find(['z', 'i']) {
            Some(i) => (&s[..i], &s[i..]),
            None => (s, ""),
        };
        let (random, fixed, seed, span) = if let Some(r) = base.strip_prefix('r') {
            match r.split_once('-') {
                Some((sd, sp)) => (true, 1, sd.parse::<u64>().ok()?, sp.parse::<u32>().ok()?),
                None => (true, 1, r.parse::<u64>().ok()?, 64),
            }
        } else {
            (false, base.strip_prefix('b')?.parse::<usize>().ok()?, 1, 64)
        };
        let mut zero_after = 0usize;
        let mut inline_final = false;
        while let Some(c) = rest.chars().next() {
            match c {
                'z' => {
                    rest = &rest[1..];
                    let end = rest.find('i').unwrap_or(rest.len());
                    zero_after = rest[..end].parse::<usize>().ok()?;
                    rest = &rest[end..];
                }
                'i' => {
                    inline_final = true;
                    rest = &rest[1..];
                }
                _ => return None,
            }
        }
        Some(ShadowPlan {
            mode: mode.to_string(),
            random,
            fixed,
            seed,
            span,
            ctor,
            zero_after,
            inline_final,
        })
    }

    /// `bN` with N >= the document length is upstream's "whole document in one
    /// non-final chunk" shape.
    pub fn label(&self) -> String {
        self.mode.clone()
    }
}

/// The probe's split generator, mirrored exactly: `next_split` over an LCG
/// seeded from the plan (`rng_state = rng_state * 6364136223846793005 +
/// 1442695040888963407`, then `(state >> 33) % span`).
struct Splitter {
    state: u64,
    random: bool,
    fixed: usize,
    span: u32,
}

impl Splitter {
    fn new(plan: &ShadowPlan) -> Self {
        Splitter {
            state: plan.seed,
            random: plan.random,
            fixed: plan.fixed,
            span: plan.span,
        }
    }

    fn next(&mut self, remaining: usize) -> usize {
        if !self.random {
            return remaining.min(self.fixed);
        }
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let n = 1 + ((self.state >> 33) % self.span as u64) as usize;
        n.min(remaining)
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
/// Every field is read from the PUBLISHED ABI window (`ctxt->input`) and the
/// context, not re-derived from the Rust buffer: the court's job is to prove
/// the driver exposes the same moving physical window libxml2 does. `rc` is
/// not a driver return value — `xmlParseChunk` returns `errNo` when the
/// document is not well-formed and 0 otherwise, so the same expression is
/// evaluated from the state the driver left behind.
unsafe fn tail_line(lbl: &str, idx: usize, ctxt: *mut _xmlParserCtxt, ctor: bool) -> String {
    let err = unsafe { (*ctxt).errNo };
    let wf = unsafe { (*ctxt).wellFormed };
    let instate = unsafe { (*ctxt).instate };
    let name_nr = unsafe { (*ctxt).nameNr };
    let input_nr = unsafe { (*ctxt).inputNr };
    let input = unsafe { (*ctxt).input };
    let head = if ctor {
        format!("< {lbl} ok=1 err={err} wf={wf} in={instate}")
    } else {
        let rc = if wf == 0 { err } else { 0 };
        format!("< {lbl} {idx} rc={rc} err={err} wf={wf} in={instate}")
    };
    if input.is_null() {
        return format!("{head} p=-1 l=-1 col=-1 i=-1 n={name_nr}");
    }
    // Candidate-side invariants (never part of the compared trace): the
    // published window must be ordered and `inputTab[0]` must be the very
    // input `ctxt->input` names, or a consumer walking the stack sees a
    // different window than the one the court just asserted against.
    debug_assert!(
        unsafe { (*input).base <= (*input).cur && (*input).cur <= (*input).end },
        "published window is not ordered: base <= cur <= end"
    );
    debug_assert!(
        unsafe { (*ctxt).inputTab.is_null() || *(*ctxt).inputTab == input },
        "inputTab[0] does not name ctxt->input"
    );
    let p = unsafe { (*input).cur.offset_from((*input).base) };
    let consumed = unsafe { (*input).consumed };
    let line = unsafe { (*input).line };
    let col = unsafe { (*input).col };
    let mut out = format!(
        "{head} p={p} l={line} col={col} i={input_nr} n={name_nr} c={consumed} abs={abs}",
        abs = consumed as i64 + p as i64
    );
    // The PUBLIC accessors over the same input, mirroring the probe's `-W`
    // fields: calling the exports (rather than re-reading the struct) proves
    // `inputTab[0]` is published — they index `inputTab`, not `ctxt->input` —
    // and `wbytes0` proves the materialized window BYTES agree, which the raw
    // pointer fields alone cannot show.
    unsafe {
        let mut line0: c_int = -1;
        let mut col0: c_int = -1;
        let mut byte0: c_ulong = 0;
        let prc = crate::abi::exports_parserint::xmlCtxtGetInputPosition(
            ctxt,
            0,
            std::ptr::null_mut(),
            &mut line0,
            &mut col0,
            &mut byte0,
        );
        out.push_str(&format!(
            " pos0={prc} line0={line0} col0={col0} byte0={byte0}"
        ));
        let mut start: *const xmlChar = std::ptr::null();
        let mut size: c_int = 80;
        let mut off: c_int = -1;
        let wrc = crate::abi::exports_parserint::xmlCtxtGetInputWindow(
            ctxt, 0, &mut start, &mut size, &mut off,
        );
        out.push_str(&format!(" win0={wrc} wsize0={size} woff0={off} wbytes0=["));
        if wrc == 0 && !start.is_null() && size > 0 {
            out.push_str(&esc_bytes(std::slice::from_raw_parts(start, size as usize)));
        }
        out.push(']');
    }
    out
}

/// Drive one document through one plan and emit the probe-grammar trace.
pub(crate) fn run_driver(doc: &[u8], plan: &ShadowPlan, name: &str) -> Vec<String> {
    let mode = plan.label();
    let mut out = vec![format!("== {name} bytes={} mode={mode}", doc.len())];
    unsafe {
        let guard = CtxtGuard(helpers::create_parser_ctxt());
        let ctxt = guard.0;
        assert!(!ctxt.is_null());
        CTXT.with(|c| c.set(ctxt));
        install_recorder(ctxt);
        // Diagnostics enter the same ordered stream as the SAX events.
        crate::abi::exports_parser::xmlCtxtSetErrorHandler(
            ctxt,
            Some(on_error),
            std::ptr::null_mut(),
        );

        // `p` borrows the raw context pointer, so it must be dropped before
        // the guard; declaring it here keeps that ordering explicit.
        let mut parser: XmlParser;
        let mut machine = PushMachine::new();
        let mut off = 0usize;
        let mut call = 0usize;
        let mut splitter = Splitter::new(plan);

        if plan.ctor && !doc.is_empty() {
            let n = splitter.next(doc.len());
            out.push(format!("> CTOR len={n}"));
            drain_events();
            let buf = InputBuffer::for_push(&doc[..n], None);
            parser = XmlParser::new_with_flags(InputStack::new(buf), ctxt, false, false);
            let _ = parser.drive_persistent(&mut machine, false);
            out.append(&mut drain_events());
            out.push(tail_line("CTOR", 0, ctxt, true));
            off = n;
        } else {
            let buf = InputBuffer::for_push(&[], None);
            parser = XmlParser::new_with_flags(InputStack::new(buf), ctxt, false, false);
        }

        while off < doc.len() {
            let n = splitter.next(doc.len() - off);
            let terminate = plan.inline_final && off + n >= doc.len();
            out.push(format!("> CALL {call} len={n}"));
            drain_events();
            let _ = parser.push_persistent(&mut machine, &doc[off..off + n], terminate);
            out.append(&mut drain_events());
            out.push(tail_line("CALL", call, ctxt, false));
            off += n;
            // The probe increments the call index BEFORE injecting the
            // zero-length calls, so their label is the NEXT index.
            call += 1;
            for _ in 0..plan.zero_after {
                out.push(format!("> CALL {call} ZERO"));
                drain_events();
                let _ = parser.push_persistent(&mut machine, &[], false);
                out.append(&mut drain_events());
                out.push(tail_line("CALL", call, ctxt, false));
            }
        }

        if !plan.inline_final {
            out.push("> FINAL".to_string());
            drain_events();
            let _ = parser.push_persistent(&mut machine, &[], true);
            out.append(&mut drain_events());
            out.push(tail_line("FINAL", 0, ctxt, false));
        }

        out.push("> REFEED".to_string());
        drain_events();
        let _ = parser.push_persistent(&mut machine, doc, true);
        out.append(&mut drain_events());
        out.push(tail_line("REFEED", 0, ctxt, false));
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
    /// For the encoder documents: whether the FULL diagnostic records match
    /// (message payload included), not merely their canonical form.
    pub encoder_full_errors_match: Option<bool>,
    /// Whether this cell uses the inline-final plan shape (the terminating flag
    /// arrives WITH the last real bytes).
    pub inline_final: bool,
    /// Whether this cell uses a deterministic-random split plan.
    pub random_plan: bool,
    pub oracle_lines: usize,
    pub driver_lines: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CellStatus {
    Match,
    Diverge,
}

// ── the compared projection ────────────────────────────────────────────

/// One `key=value` field out of a space-separated record tail.
fn field(s: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=");
    let i = s.find(&pat)? + pat.len();
    let rest = &s[i..];
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Canonical form of a structured diagnostic: identity and position, without
/// the message window (`file`/`str1..3`/`msg`), which is the existing error
/// courts' surface. Occurrence, code and ORDERING are what this court asserts.
fn canonical_error(line: &str) -> String {
    let rest = line.strip_prefix("error ").unwrap_or(line);
    let mut out = String::from("error");
    for key in ["dom", "code", "level", "line", "i1", "i2"] {
        if let Some(v) = field(rest, key) {
            out.push_str(&format!(" {key}={v}"));
        }
    }
    out
}

/// The line sequence the court asserts on.
///
/// One exclusion, deliberate and documented in the module docs: structured
/// diagnostics are reduced to their CANONICAL form (domain, code, level, line,
/// int1, int2) rather than dropped, so `endDocument` before `error 81` is
/// distinguishable from `error 81` before `endDocument` — and once
/// declarations/DTDs arrive a single parse path can emit several. The two
/// ENCODER documents additionally assert the full records.
///
/// `p`, `i`, `c` and `abs` are now ASSERTED: they come from the published ABI
/// window, so the court proves the driver exposes libxml2's moving physical
/// window rather than merely a logical position.
fn project(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            if l.starts_with("error ") {
                canonical_error(l)
            } else {
                l.clone()
            }
        })
        .collect()
}

/// The full diagnostic records, in order. Used for the encoder pair, whose
/// records are stable and are the contract those documents exist to pin.
fn error_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|l| l.starts_with("error "))
        .cloned()
        .collect()
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
        let encoder_full_errors_match = if ENCODER_DOCS.contains(&doc_name) {
            Some(error_lines(&oracle) == error_lines(&driver))
        } else {
            None
        };
        reports.push(CellReport {
            doc: doc_name.to_string(),
            mode: mode.to_string(),
            status,
            first_diff,
            encoder_full_errors_match,
            inline_final: plan.inline_final,
            random_plan: plan.random,
            oracle_lines: oracle.len(),
            driver_lines: driver.len(),
        });
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cell must match the oracle. There is no known-red allowlist any
    /// more: the encoder-flush and definite-invalid cells this court was built
    /// around now pass, and an allowlist would let them regress silently.
    ///
    /// Ignored by default only because it needs the committed oracle fixtures
    /// (generated by `pushdrive-shadow-run.sh`); run it with
    /// `cargo test --lib pushshadow -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs the committed oracle fixtures; run explicitly"]
    fn shadow_court_report() {
        let reports = run_all();
        assert!(!reports.is_empty(), "no shadow cells found");
        assert_shadow_matrix(&reports);

        let diverged: Vec<_> = reports
            .iter()
            .filter(|r| r.status == CellStatus::Diverge)
            .collect();
        println!(
            "shadow: {} cells, {} match, {} diverge",
            reports.len(),
            reports.len() - diverged.len(),
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

        // Shape guards: the cells that matter most must be in the court, so a
        // missing fixture cannot masquerade as a pass. (`assert_shadow_matrix`
        // pins the exact matrix; these pin the shape properties.)
        for doc in [
            "shadow-utf16trunc.xml",
            "shadow-utf16invalid.xml",
            "shadow-longtext.xml",
            "shadow-ns.xml",
        ] {
            assert!(
                reports.iter().any(|r| r.doc == doc),
                "shadow court is missing every cell for {doc}"
            );
        }
        assert!(
            reports.iter().any(|r| r.inline_final),
            "shadow court has no inline-final cell"
        );
        assert!(
            reports.iter().any(|r| r.random_plan),
            "shadow court has no random-plan cell"
        );
        // The encoder pair's FULL diagnostic records, message payload
        // included — the contract those two documents exist to pin.
        for r in reports
            .iter()
            .filter(|r| ENCODER_DOCS.contains(&r.doc.as_str()))
        {
            assert_eq!(
                r.encoder_full_errors_match,
                Some(true),
                "full encoder diagnostic record diverged for {} [{}]",
                r.doc,
                r.mode
            );
        }

        assert!(
            diverged.is_empty(),
            "shadow court diverged: {:#?}",
            diverged
                .iter()
                .map(|r| format!("{} [{}]", r.doc, r.mode))
                .collect::<Vec<_>>()
        );
    }

    /// The fixture matrix must be EXACTLY the expected one.
    ///
    /// The plan sets are declared here rather than read from the shell
    /// launcher on purpose: if a mode is dropped from `pushdrive-shadow-run.sh`
    /// and the fixtures are regenerated, this must FAIL rather than quietly
    /// accept a narrower court. The threshold that selects the small plan set
    /// duplicates the launcher's for the same reason.
    fn assert_shadow_matrix(reports: &[CellReport]) {
        let mut docs: Vec<(String, u64)> = std::fs::read_dir(CORPUS_DIR)
            .expect("shadow corpus missing")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "xml"))
            .map(|e| {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                (e.file_name().to_string_lossy().into_owned(), size)
            })
            .collect();
        docs.sort();

        let mut expected_total = 0usize;
        for (doc, size) in &docs {
            let expected: Vec<String> = if doc.starts_with("shadow-win-") {
                WINDOW_MODES.iter().map(|s| s.to_string()).collect()
            } else if *size <= SMALL_DOC_MAX as u64 {
                SMALL_MODES.iter().map(|s| s.to_string()).collect()
            } else {
                LONG_MODES.iter().map(|s| s.to_string()).collect()
            };
            let mut actual: Vec<String> = reports
                .iter()
                .filter(|r| &r.doc == doc)
                .map(|r| r.mode.clone())
                .collect();
            actual.sort();
            let mut want = expected.clone();
            want.sort();
            assert_eq!(
                actual, want,
                "shadow fixture matrix mismatch for {doc} ({size} bytes)"
            );
            expected_total += expected.len();
        }

        assert_eq!(
            reports.len(),
            expected_total,
            "shadow cell total does not match the expected matrix"
        );
        assert_eq!(
            reports.len(),
            SHADOW_CELL_TOTAL,
            "the shadow cell total changed — update the matrix deliberately, not silently"
        );
    }
}
