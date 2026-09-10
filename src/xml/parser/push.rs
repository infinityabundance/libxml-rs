//! Persistent progressive-push machine — §16.7.8 slice 1 foundation.
//!
//! This module holds the state a push parser must keep ALIVE across
//! `xmlParseChunk` calls. The replay architecture it replaces had no place
//! to put that state, so every call re-parsed the whole accumulated buffer
//! (silent probe + delivery reparse), which is both O(N²) and the source of
//! the push divergence classes the §16.7.8 differential court froze
//! (`courts/receipts/phase-16/16-7-8-pushdiff-court.md`):
//!
//! 1. lifecycle / event timing (replayed startDocument/endDocument),
//! 2. `NeedMoreInput` treated as EOF/malformed input,
//! 3. trailing-CR `end_in_lf` deferral,
//! 4. finished-context REFEED behavior,
//! 5. character-data segmentation (raw-CR/CRLF + the ≥300-byte
//!    availability gate — a property of the progressive input buffer).
//!
//! The invariant the whole design rests on:
//!
//! > **byte exhaustion with `terminate == 0` is SUSPENSION, never EOF.**
//!
//! A non-final call that runs out of bytes returns
//! [`PushProgress::NeedMoreInput`] and parks; the next call appends bytes to
//! the SAME machine — same cursor, same phase, same stacks — and resumes.
//! Bytes are consumed at most once: [`PushMachine::bytes_consumed`] never
//! exceeds [`PushMachine::bytes_appended`].
//!
//! # Contract for the driver (wiring step)
//!
//! - **Termination is an input to a pass, not just bytes.** A zero-length
//!   terminating call (`xmlParseChunk(ctxt, NULL, 0, 1)`) has no unread
//!   bytes but is fully active: it may finish the document, fire
//!   `endDocument`, report an unfinished tag or empty document, or move
//!   EPILOG → EOF. [`PushMachine::resume`] therefore takes `terminate` and
//!   never reports `NeedMoreInput` when `terminate` is true.
//! - **Absolute vs physical position.** [`PushMachine::bytes_consumed`] is
//!   an ABSOLUTE, monotonic stream offset (for O(N) accounting). The
//!   ABI-visible `ctxt->input->cur - ctxt->input->base` is NOT this value:
//!   the physical buffer is rebased by `xmlParserShrink` whenever
//!   `cur - base > 4096`, so it is owned by the input buffer, never by this
//!   machine. Do not conflate them.
//! - **No mid-stream fallback.** A context must be committed to the
//!   persistent path BEFORE its first immutable observable state (any SAX
//!   event) is emitted, or not at all. Starting persistent and retreating
//!   to replay mid-document would duplicate events/DOM/name state. During
//!   development the persistent path is exercised by dedicated tests;
//!   `parse_chunk` stays on replay until the persistent grammar covers a
//!   coherent subset, then whole contexts flip over.
//! - **Locking.** The machine lives in the per-context side table. Its
//!   global map mutex must NEVER be held across `resume()`/SAX callbacks
//!   (that would serialize every push parser and risk callback reentrancy).
//!   Take the machine out, release the map lock, run, then put it back.

use crate::abi::types::xmlParserInputState;

/// Outcome of one `resume()` pass over the bytes currently available.
///
/// The distinction between [`NeedMoreInput`](PushProgress::NeedMoreInput)
/// and [`DocumentComplete`](PushProgress::DocumentComplete)/[`Fatal`](PushProgress::Fatal)
/// is the semantic heart of the progressive parser: an incomplete construct
/// at the end of the available bytes is SUSPENSION (the bytes may arrive in
/// a later chunk), not a truncated document and not an error. Upstream
/// expresses this with `xmlParseTryOrFinish`'s `goto done` on non-final
/// calls (`avail < 1`, `xmlParseLookupGt`/`xmlParseLookupCharData` returning
/// 0, the `!terminate` gates), never by raising.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PushProgress {
    /// Bytes were consumed and events may have fired; call `resume()` again.
    Progress,
    /// The available bytes are exhausted inside an incomplete construct
    /// (non-final call): park and wait for more input. NEVER returned for a
    /// terminating pass.
    NeedMoreInput,
    /// The document reached its end (upstream `XML_PARSER_EPILOG`/`EOF`
    /// after `xmlFinishDocument`, or a clean end observed with
    /// `terminate != 0`).
    DocumentComplete,
    /// A fatal error was raised, or the parser was stopped
    /// (`wellFormed == 0` / `disableSAX != 0`); later chunks are refused
    /// with the recorded `errNo`.
    Fatal,
}

/// One open element, parked across calls so the next chunk resumes the
/// content loop without replaying the prefix. Mirrors the engine's
/// `OpenElement` (name, start line, namespace-scope mark).
#[derive(Clone, Debug)]
#[allow(dead_code)] // populated when the driver takes over element frames
pub(crate) struct ParkedElement {
    pub name: Vec<u8>,
    pub line: usize,
    pub ns_scope_mark: usize,
}

/// Persistent push state: lives in the context's side table for the whole
/// push session, across `xmlParseChunk` calls and zero-length calls, and is
/// dropped by `free_push_state` (so `xmlCtxtReset` cannot leak it into the
/// next document).
#[derive(Debug)]
pub(crate) struct PushMachine {
    /// ABSOLUTE, monotonic stream offset of the first byte not yet consumed
    /// (O(N) accounting). This is NOT the ABI-visible `cur - base`; see the
    /// module docs.
    bytes_consumed: u64,
    /// Bytes appended by the caller over the session.
    bytes_appended: u64,
    /// Scanner work: byte inspections / lookahead operations, which may
    /// exceed the consumed count (peek, classify, look ahead, consume). The
    /// complexity property to prove is "no whole-prefix restart", i.e.
    /// `scan_work / bytes_appended` stays a bounded constant — not that it
    /// equals 1.0.
    scan_work: u64,
    /// 1-based line of the consumed position (upstream `input->line`).
    line: usize,
    /// 1-based column of the consumed position (upstream `input->col`).
    column: usize,
    /// Current document phase — EXACTLY the ABI-visible
    /// `xmlParserInputState` upstream keeps in `ctxt->instate` (all 19
    /// states), so the machine never maintains a lossy parallel enum.
    phase: xmlParserInputState,
    /// Open elements, outermost first — the resume point for content.
    open_elements: Vec<ParkedElement>,
    /// In-scope namespace bindings (prefix, href), outermost first;
    /// `ParkedElement::ns_scope_mark` indexes into it.
    ns_scope: Vec<(Vec<u8>, Vec<u8>)>,
    /// `startDocument` fired exactly once, when the phase leaves `START`.
    start_document_fired: bool,
    /// `endDocument` fired exactly once — on the terminating call (or a
    /// fatal), never merely because a complete document sits in a non-final
    /// chunk.
    end_document_fired: bool,
    /// A fatal error was raised; later chunks are refused with `errNo`.
    fatal: bool,
    /// `xmlStopParser` was called from a callback: refuse further input.
    stopped: bool,
}

impl Default for PushMachine {
    /// NOTE: manual impl — line/column start at 1 (upstream input->line and
    /// input->col are 1-based). A derived `Default` would silently start
    /// them at 0, which is exactly the kind of foundation bug that shows up
    /// only after wiring.
    fn default() -> Self {
        Self {
            bytes_consumed: 0,
            bytes_appended: 0,
            scan_work: 0,
            line: 1,
            column: 1,
            phase: xmlParserInputState::XML_PARSER_START,
            open_elements: Vec::new(),
            ns_scope: Vec::new(),
            start_document_fired: false,
            end_document_fired: false,
            fatal: false,
            stopped: false,
        }
    }
}

#[allow(dead_code)] // driver wiring lands in the next step
impl PushMachine {
    /// A fresh machine at `XML_PARSER_START` with line/column 1.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Append `len` newly received bytes. Zero-length calls (including
    /// `xmlParseChunk(NULL, 0, 0)`) append nothing and must not advance any
    /// state by themselves — they only re-run `resume()` over what is
    /// already buffered (upstream's `xmlParseTryOrFinish` over the same
    /// buffer), which is why the court's `zK` plans exist.
    pub(crate) fn append(&mut self, len: usize) {
        self.bytes_appended = self.bytes_appended.saturating_add(len as u64);
    }

    /// Record that `n` bytes have been consumed (the absolute cursor
    /// advances past them).
    pub(crate) fn note_consumed(&mut self, n: usize) {
        self.bytes_consumed = self.bytes_consumed.saturating_add(n as u64);
    }

    /// Record scanner work (byte inspections/lookahead) for the complexity
    /// receipt. This is deliberately separate from `note_consumed`: peeking
    /// and classifying are work but not consumption.
    pub(crate) fn note_scan_work(&mut self, n: usize) {
        self.scan_work = self.scan_work.saturating_add(n as u64);
    }

    /// Bytes appended but not yet consumed.
    pub(crate) fn unread(&self) -> u64 {
        self.bytes_appended.saturating_sub(self.bytes_consumed)
    }

    /// Run one pass. `terminate` is the `xmlParseChunk` final flag.
    ///
    /// This is the state-level decision only: it says what a pass MUST be
    /// able to do, not what it produces. The driver's scanner/terminator
    /// logic turns `Progress` into events/errors (and, for an empty
    /// terminating call, into upstream's terminator checks:
    /// `XML_ERR_TAG_NOT_FINISHED`, `XML_ERR_DOCUMENT_EMPTY`,
    /// `xmlParserCheckEOF`).
    pub(crate) fn resume(&mut self, terminate: bool) -> PushProgress {
        // A stopped/fatal context refuses without parsing: upstream gates on
        // `disableSAX != 0` / the EOF latch and returns the recorded errNo.
        if self.fatal || self.stopped {
            return PushProgress::Fatal;
        }
        if self.phase == xmlParserInputState::XML_PARSER_EOF {
            return PushProgress::DocumentComplete;
        }
        // Unread bytes OR a terminating call: a pass is possible. A
        // zero-length NON-final call with nothing unread is the one case
        // that cannot progress — and must NOT be treated as EOF (class 2).
        if self.unread() > 0 || terminate {
            return PushProgress::Progress;
        }
        PushProgress::NeedMoreInput
    }

    pub(crate) const fn phase(&self) -> xmlParserInputState {
        self.phase
    }

    pub(crate) fn set_phase(&mut self, phase: xmlParserInputState) {
        self.phase = phase;
    }

    pub(crate) const fn bytes_consumed(&self) -> u64 {
        self.bytes_consumed
    }

    pub(crate) const fn bytes_appended(&self) -> u64 {
        self.bytes_appended
    }

    pub(crate) const fn scan_work(&self) -> u64 {
        self.scan_work
    }

    pub(crate) const fn line(&self) -> usize {
        self.line
    }

    pub(crate) const fn column(&self) -> usize {
        self.column
    }

    pub(crate) fn set_position(&mut self, line: usize, column: usize) {
        self.line = line;
        self.column = column;
    }

    pub(crate) fn open_elements(&self) -> &[ParkedElement] {
        &self.open_elements
    }

    pub(crate) fn push_element(&mut self, name: Vec<u8>, line: usize, ns_scope_mark: usize) {
        self.open_elements.push(ParkedElement {
            name,
            line,
            ns_scope_mark,
        });
    }

    pub(crate) fn pop_element(&mut self) -> Option<ParkedElement> {
        self.open_elements.pop()
    }

    pub(crate) const fn start_document_fired(&self) -> bool {
        self.start_document_fired
    }

    pub(crate) fn mark_start_document_fired(&mut self) {
        self.start_document_fired = true;
    }

    pub(crate) const fn end_document_fired(&self) -> bool {
        self.end_document_fired
    }

    pub(crate) fn mark_end_document_fired(&mut self) {
        self.end_document_fired = true;
    }

    /// Latch a fatal error (`wellFormed == 0` / `disableSAX != 0`).
    pub(crate) fn mark_fatal(&mut self) {
        self.fatal = true;
    }

    pub(crate) const fn is_fatal(&self) -> bool {
        self.fatal
    }

    /// Latch `xmlStopParser` (disableSAX = 2): every later chunk is refused
    /// with the recorded error without parsing anything.
    pub(crate) fn mark_stopped(&mut self) {
        self.stopped = true;
    }

    pub(crate) const fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Instrumentation gate: the O(N) invariant — no byte is consumed
    /// twice (the absolute cursor never moves backward).
    pub(crate) fn consumed_le_appended(&self) -> bool {
        self.bytes_consumed <= self.bytes_appended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn need_more_input_is_not_eof() {
        // The semantic contract: suspension and document end are distinct
        // outcomes; conflating them is divergence class 2.
        assert_ne!(PushProgress::NeedMoreInput, PushProgress::DocumentComplete);
        assert_ne!(PushProgress::NeedMoreInput, PushProgress::Fatal);
    }

    #[test]
    fn zero_length_non_final_call_cannot_progress() {
        let mut m = PushMachine::new();
        m.append(0);
        assert_eq!(m.unread(), 0);
        assert_eq!(m.resume(false), PushProgress::NeedMoreInput);
        // ...and a zero-length call after real input parks the same way
        // until the bytes are consumed.
        m.append(3);
        assert_eq!(m.resume(false), PushProgress::Progress);
        m.note_consumed(3);
        assert_eq!(m.resume(false), PushProgress::NeedMoreInput);
    }

    #[test]
    fn terminating_call_is_active_with_zero_unread_bytes() {
        // The three zero-byte-FINAL shapes that must never be reported as
        // NeedMoreInput: a complete doc resting at EPILOG, an unfinished
        // element, and an empty document. The driver's terminator checks
        // decide the concrete outcome; the state machine must let them run.
        for (phase, appended, consumed) in [
            (xmlParserInputState::XML_PARSER_EPILOG, 4u64, 4u64), // <a/>
            (xmlParserInputState::XML_PARSER_CONTENT, 3, 3),      // <a>
            (xmlParserInputState::XML_PARSER_START, 0, 0),        // empty
        ] {
            let mut m = PushMachine::new();
            m.set_phase(phase);
            m.append(appended as usize);
            m.note_consumed(consumed as usize);
            assert_eq!(m.unread(), 0);
            assert_ne!(
                m.resume(true),
                PushProgress::NeedMoreInput,
                "terminate=true must never be NeedMoreInput (phase {:?})",
                phase
            );
        }
    }

    #[test]
    fn zero_length_calls_do_not_advance_state() {
        let mut m = PushMachine::new();
        m.append(0);
        assert_eq!(m.bytes_appended(), 0);
        assert_eq!(m.bytes_consumed(), 0);
        assert_eq!(m.scan_work(), 0);
        // A zero-length non-final call after real input re-runs over the
        // same buffer: the cursor parks where the last pass stopped.
        m.append(3);
        m.note_consumed(2);
        m.append(0);
        assert_eq!(m.bytes_consumed(), 2, "zero-length call consumed nothing");
        assert_eq!(m.unread(), 1);
    }

    #[test]
    fn consumed_never_exceeds_appended() {
        let mut m = PushMachine::new();
        m.append(10);
        m.note_consumed(4);
        assert!(m.consumed_le_appended());
        // A resumed pass continues from the parked cursor; it does not
        // re-consume the first four bytes.
        m.append(2);
        m.note_consumed(2);
        assert_eq!(m.bytes_consumed(), 6);
        assert_eq!(m.bytes_appended(), 12);
        assert_eq!(m.unread(), 6);
        assert!(m.consumed_le_appended());
    }

    #[test]
    fn scan_work_is_separate_from_consumption() {
        // Peeking/classifying is work but not consumption: the complexity
        // property is a bounded scan_work/appended ratio, not equality.
        let mut m = PushMachine::new();
        m.append(4);
        m.note_scan_work(9); // e.g. lookahead across a construct
        m.note_consumed(4);
        assert_eq!(m.bytes_consumed(), 4);
        assert_eq!(m.scan_work(), 9);
        assert!(m.consumed_le_appended());
    }

    #[test]
    fn phase_is_the_abi_enum() {
        // All 19 upstream states are representable because the machine uses
        // the ABI type directly (no lossy parallel enum).
        let m = PushMachine::new();
        assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_START);
        for p in [
            xmlParserInputState::XML_PARSER_EOF,
            xmlParserInputState::XML_PARSER_PI,
            xmlParserInputState::XML_PARSER_COMMENT,
            xmlParserInputState::XML_PARSER_CDATA_SECTION,
            xmlParserInputState::XML_PARSER_ENTITY_DECL,
            xmlParserInputState::XML_PARSER_ENTITY_VALUE,
            xmlParserInputState::XML_PARSER_ATTRIBUTE_VALUE,
            xmlParserInputState::XML_PARSER_SYSTEM_LITERAL,
            xmlParserInputState::XML_PARSER_IGNORE,
            xmlParserInputState::XML_PARSER_PUBLIC_LITERAL,
            xmlParserInputState::XML_PARSER_XML_DECL,
        ] {
            let mut m2 = PushMachine::new();
            m2.set_phase(p);
            assert_eq!(m2.phase(), p);
        }
    }

    #[test]
    fn default_starts_at_line_and_column_one() {
        let m = PushMachine::default();
        assert_eq!(m.line(), 1);
        assert_eq!(m.column(), 1);
        let n = PushMachine::new();
        assert_eq!(n.line(), 1);
        assert_eq!(n.column(), 1);
    }

    #[test]
    fn open_element_stack_survives_across_calls() {
        let mut m = PushMachine::new();
        m.push_element(b"root".to_vec(), 1, 0);
        m.push_element(b"child".to_vec(), 1, 0);
        assert_eq!(m.open_elements().len(), 2);
        assert_eq!(m.pop_element().unwrap().name, b"child");
        assert_eq!(m.open_elements().len(), 1, "state persists, not replayed");
    }

    #[test]
    fn stop_and_fatal_refuse_further_input() {
        let mut m = PushMachine::new();
        m.append(5);
        assert_eq!(m.resume(false), PushProgress::Progress);
        m.mark_stopped();
        assert_eq!(m.resume(false), PushProgress::Fatal);
        let mut m2 = PushMachine::new();
        m2.append(5);
        m2.mark_fatal();
        assert_eq!(m2.resume(false), PushProgress::Fatal);
        // ...and even a terminating call is refused.
        assert_eq!(m2.resume(true), PushProgress::Fatal);
    }

    #[test]
    fn eof_phase_reports_document_complete() {
        let mut m = PushMachine::new();
        m.append(4);
        m.note_consumed(4);
        m.set_phase(xmlParserInputState::XML_PARSER_EOF);
        assert_eq!(m.resume(false), PushProgress::DocumentComplete);
        assert_eq!(m.resume(true), PushProgress::DocumentComplete);
    }

    #[test]
    fn lifecycle_flags_fire_once() {
        let mut m = PushMachine::new();
        assert!(!m.start_document_fired() && !m.end_document_fired());
        m.mark_start_document_fired();
        m.append(4);
        assert!(m.start_document_fired());
        assert!(!m.end_document_fired(), "endDocument waits for terminate");
        m.mark_end_document_fired();
        assert!(m.end_document_fired());
    }
}
