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
//! - **Absolute vs physical position.** [`PushMachine::base_bytes_consumed`]
//!   is an ABSOLUTE, monotonic offset into the BASE push stream (for O(N)
//!   accounting). The ABI-visible `ctxt->input->cur - ctxt->input->base` is
//!   NOT this value: the physical buffer is rebased by `xmlParserShrink`
//!   whenever `cur - base > 4096`, and it is owned by the input buffer.
//!   Likewise **line/column belong to the input**, not to this machine:
//!   `InputBuffer`/`InputStack` owns `pos/line/col` per input, which is the
//!   only model that survives entity content (an entity input has its own
//!   cur/line/col; popping returns to the parent input's position). This
//!   machine keeps no position fields of its own.
//! - **Accounting domains.** Three byte domains are kept distinct so units
//!   are never mixed:
//!   - `source_bytes_received` — RAW bytes supplied through
//!     `xmlParseChunk` (recorded for source throughput);
//!   - `input_bytes_materialized` — UTF-8/internal bytes the input buffer
//!     presents to the scanner (transcoding can EXPAND or shrink these:
//!     one ISO-8859-1 `E9` byte materializes as `C3 A9`);
//!   - `input_bytes_consumed` — same unit as materialized;
//!   - `total_scan_work` — inspections/lookahead, including bytes examined
//!     from expanded entity inputs.
//!   The invariant is `input_bytes_consumed <= input_bytes_materialized`
//!   (NOT consumption <= raw source bytes, which transcoding would break),
//!   and the complexity metric is bounded `total_scan_work /
//!   input_bytes_materialized`. Violating the invariant latches
//!   [`PushMachine::accounting_violation`].
//! - **No mid-stream fallback.** A context must be committed to the
//!   persistent path BEFORE its first immutable observable state (any SAX
//!   event) is emitted, or not at all. Starting persistent and retreating
//!   to replay mid-document would duplicate events/DOM/name state. During
//!   development the persistent path is exercised by dedicated tests;
//!   `parse_chunk` stays on replay until the persistent grammar covers a
//!   coherent subset, then whole contexts flip over.
//! - **Reuse, don't fork, the grammar.** The driver must reuse the existing
//!   tokenizer/parsing primitives; `state.rs` must not become the pull
//!   parser while `push.rs` grows into a second, independently maintained
//!   XML parser. Same XML semantics and the same SAX/DOM machinery — only
//!   the execution model changes (persistent/resumable instead of
//!   recursive whole-document replay).
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
    /// The document is finished (`XML_PARSER_EOF`) but unread bytes remain
    /// from a non-final call.
    ///
    /// Upstream `xmlParseTryOrFinish` hits `case XML_PARSER_EOF: goto done`
    /// immediately, so it consumes nothing and raises nothing — the bytes
    /// stay buffered and the call returns 0. Only a later TERMINATING call
    /// runs `xmlParserCheckEOF`, which sees `cur < end` and raises
    /// `XML_ERR_DOCUMENT_END` (divergence class 4's REFEED). A driver must
    /// therefore never loop on this outcome (it would spin forever); it
    /// parks and waits for termination.
    AwaitTermination,
    /// The document reached its end and nothing is unread (upstream
    /// `XML_PARSER_EOF` with no leftover input).
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
    /// RAW bytes supplied by the caller through `xmlParseChunk` (recorded
    /// for source throughput only — NOT the unit of the invariant).
    source_bytes_received: u64,
    /// UTF-8/internal bytes the input buffer presents to the scanner
    /// (transcoding may expand or shrink: ISO-8859-1 `E9` -> `C3 A9`).
    input_bytes_materialized: u64,
    /// Bytes consumed by the scanner — same unit as materialized.
    input_bytes_consumed: u64,
    /// Scanner work: byte inspections / lookahead operations, which may
    /// exceed the consumed count (peek, classify, look ahead, consume), and
    /// which include bytes examined from expanded entity inputs that were
    /// never appended through `xmlParseChunk`. The complexity property to
    /// prove is "no whole-prefix restart", i.e. `total_scan_work /
    /// input_bytes_materialized` stays a bounded constant — not that it
    /// equals 1.0.
    total_scan_work: u64,
    /// Set when consumption is advanced past the materialized input (an
    /// accounting bug, e.g. rescanning a prefix); makes the violation
    /// visible instead of silently clamping `unread()` to 0.
    accounting_violation: bool,
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
    /// NOTE: manual impl. Position (line/column) deliberately does NOT live
    /// here — the input owns it (see the module docs); this struct only
    /// defaults to `XML_PARSER_START` with zero accounting.
    fn default() -> Self {
        Self {
            source_bytes_received: 0,
            input_bytes_materialized: 0,
            input_bytes_consumed: 0,
            total_scan_work: 0,
            accounting_violation: false,
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

    /// Record `len` RAW bytes received from the caller (`xmlParseChunk`).
    /// Zero-length calls (including `xmlParseChunk(NULL, 0, 0)`) receive
    /// nothing and must not advance any state by themselves — they only
    /// re-run `resume()` over what is already buffered (upstream's
    /// `xmlParseTryOrFinish` over the same buffer), which is why the court's
    /// `zK` plans exist.
    pub(crate) fn receive_source(&mut self, len: usize) {
        self.source_bytes_received = self.source_bytes_received.saturating_add(len as u64);
    }

    /// Record that `n` internal (UTF-8) bytes became available to the
    /// scanner. For an untranscoded UTF-8 stream this equals the received
    /// source bytes; for a transcoded stream it need not (ISO-8859-1 `E9`
    /// materializes as two bytes).
    pub(crate) fn materialize_input(&mut self, n: usize) {
        self.input_bytes_materialized = self.input_bytes_materialized.saturating_add(n as u64);
    }

    /// Record that `n` internal bytes have been consumed by the scanner.
    /// Advancing past what was materialized is an accounting bug (e.g. a
    /// prefix rescan): it latches [`Self::accounting_violation`] rather than
    /// silently clamping `unread()` to zero. The violation is deliberately a
    /// STATE, not a panic/`debug_assert`: it stays inspectable (and a
    /// malformed consumer cannot be aborted by an internal bookkeeping bug).
    pub(crate) fn note_consumed(&mut self, n: usize) {
        let new = self.input_bytes_consumed.saturating_add(n as u64);
        if new > self.input_bytes_materialized {
            self.accounting_violation = true;
        }
        self.input_bytes_consumed = new;
    }

    /// Record scanner work (byte inspections/lookahead, including bytes
    /// examined from expanded entity inputs) for the complexity receipt.
    /// Deliberately separate from `note_consumed`: peeking and classifying
    /// are work but not consumption.
    pub(crate) fn note_scan_work(&mut self, n: usize) {
        self.total_scan_work = self.total_scan_work.saturating_add(n as u64);
    }

    /// Materialized internal bytes not yet consumed.
    pub(crate) fn unread(&self) -> u64 {
        self.input_bytes_materialized
            .saturating_sub(self.input_bytes_consumed)
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
        // EOF handling comes first, because in XML_PARSER_EOF upstream's
        // xmlParseTryOrFinish does nothing at all (`case XML_PARSER_EOF:
        // goto done`) regardless of unread bytes. With nothing unread the
        // document is simply complete. With leftover (refed) bytes and a
        // NON-final call there is no progress and no error yet: the bytes
        // stay buffered and the caller must park (NOT loop — this returns
        // AwaitTermination precisely so a `while Progress` driver cannot
        // spin). Only a TERMINATING call proceeds, so the finalizer can
        // observe `cur < end` and raise XML_ERR_DOCUMENT_END — divergence
        // class 4's REFEED, where the unread bytes are the evidence and are
        // never consumed.
        if self.phase == xmlParserInputState::XML_PARSER_EOF {
            if self.unread() == 0 {
                return PushProgress::DocumentComplete;
            }
            if !terminate {
                return PushProgress::AwaitTermination;
            }
            return PushProgress::Progress;
        }
        // Non-EOF: unread bytes OR a terminating call make a pass possible.
        // A zero-length NON-final call with nothing unread is the one case
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

    pub(crate) const fn source_bytes_received(&self) -> u64 {
        self.source_bytes_received
    }

    pub(crate) const fn input_bytes_materialized(&self) -> u64 {
        self.input_bytes_materialized
    }

    pub(crate) const fn input_bytes_consumed(&self) -> u64 {
        self.input_bytes_consumed
    }

    pub(crate) const fn total_scan_work(&self) -> u64 {
        self.total_scan_work
    }

    /// Whether an accounting bug advanced consumption past the materialized
    /// input (e.g. a prefix rescan). Never expected to be true.
    pub(crate) const fn accounting_violation(&self) -> bool {
        self.accounting_violation
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
    pub(crate) fn consumed_le_materialized(&self) -> bool {
        self.input_bytes_consumed <= self.input_bytes_materialized
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
        assert_ne!(PushProgress::NeedMoreInput, PushProgress::AwaitTermination);
    }

    #[test]
    fn zero_length_non_final_call_cannot_progress() {
        let mut m = PushMachine::new();
        m.receive_source(0);
        m.materialize_input(0);
        assert_eq!(m.unread(), 0);
        assert_eq!(m.resume(false), PushProgress::NeedMoreInput);
        // ...and a zero-length call after real input parks the same way
        // once the bytes are consumed.
        m.receive_source(3);
        m.materialize_input(3);
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
        for (phase, bytes) in [
            (xmlParserInputState::XML_PARSER_EPILOG, 4u64), // <a/>
            (xmlParserInputState::XML_PARSER_CONTENT, 3),   // <a>
            (xmlParserInputState::XML_PARSER_START, 0),     // empty
        ] {
            let mut m = PushMachine::new();
            m.set_phase(phase);
            m.receive_source(bytes as usize);
            m.materialize_input(bytes as usize);
            m.note_consumed(bytes as usize);
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
    fn refeed_after_eof_parks_then_finalizes_with_bytes_unread() {
        // Divergence class 4, modeled on the actual upstream lifecycle:
        // xmlParseTryOrFinish does NOTHING in XML_PARSER_EOF (goto done), so
        // the refed bytes stay unread (no consumption, no error) until a
        // terminating call lets xmlParserCheckEOF see cur < end and raise
        // XML_ERR_DOCUMENT_END. Consuming the bytes would model the wrong
        // semantics and hide the evidence the finalizer depends on.
        let mut m = PushMachine::new();
        m.receive_source(4);
        m.materialize_input(4);
        m.note_consumed(4);
        m.set_phase(xmlParserInputState::XML_PARSER_EOF);
        assert_eq!(m.resume(true), PushProgress::DocumentComplete);

        // REFEED: four more bytes on a finished context.
        m.receive_source(4);
        m.materialize_input(4);
        assert_eq!(m.unread(), 4);

        // Non-final: no progress at all, bytes stay buffered, no error yet.
        assert_eq!(m.resume(false), PushProgress::AwaitTermination);
        assert_eq!(m.unread(), 4, "EOF consumes nothing (goto done)");
        assert_eq!(m.input_bytes_consumed(), 4);

        // Several non-final calls still make no progress and must not spin.
        assert_eq!(m.resume(false), PushProgress::AwaitTermination);
        assert_eq!(m.unread(), 4);

        // Terminating: the finalizer must run (and will raise
        // XML_ERR_DOCUMENT_END); the bytes are STILL unread — they are the
        // evidence for the error.
        assert_eq!(m.resume(true), PushProgress::Progress);
        assert_eq!(m.unread(), 4, "finalization must not consume them");
    }

    #[test]
    fn zero_length_calls_do_not_advance_state() {
        let mut m = PushMachine::new();
        m.receive_source(0);
        m.materialize_input(0);
        assert_eq!(m.source_bytes_received(), 0);
        assert_eq!(m.input_bytes_materialized(), 0);
        assert_eq!(m.input_bytes_consumed(), 0);
        assert_eq!(m.total_scan_work(), 0);
        // A zero-length non-final call after real input re-runs over the
        // same buffer: the cursor parks where the last pass stopped.
        m.receive_source(3);
        m.materialize_input(3);
        m.note_consumed(2);
        m.receive_source(0);
        m.materialize_input(0);
        assert_eq!(
            m.input_bytes_consumed(),
            2,
            "zero-length call consumed nothing"
        );
        assert_eq!(m.unread(), 1);
    }

    #[test]
    fn consumed_never_exceeds_materialized() {
        let mut m = PushMachine::new();
        m.receive_source(10);
        m.materialize_input(10);
        m.note_consumed(4);
        assert!(m.consumed_le_materialized());
        // A resumed pass continues from the parked cursor; it does not
        // re-consume the first four bytes.
        m.receive_source(2);
        m.materialize_input(2);
        m.note_consumed(2);
        assert_eq!(m.input_bytes_consumed(), 6);
        assert_eq!(m.input_bytes_materialized(), 12);
        assert_eq!(m.unread(), 6);
        assert!(!m.accounting_violation());
    }

    #[test]
    fn accounting_violation_is_visible_not_clamped() {
        // Advancing consumption past the materialized input is a bug
        // (e.g. a prefix rescan). `unread()` would clamp to 0; the latch
        // makes the violation observable instead of hiding it.
        let mut m = PushMachine::new();
        m.receive_source(100);
        m.materialize_input(100);
        m.note_consumed(101);
        assert!(m.accounting_violation());
        assert_eq!(m.unread(), 0);
        assert!(!m.consumed_le_materialized());
    }

    #[test]
    fn transcode_expansion_is_not_a_violation() {
        // 1 ISO-8859-1 'é' (E9) materializes as 2 UTF-8 bytes (C3 A9):
        // consumption is measured in MATERIALIZED bytes, so 2 consumed from
        // 1 received source byte, and 2 materialized, is perfectly correct.
        let mut m = PushMachine::new();
        m.receive_source(1);
        m.materialize_input(2);
        m.note_consumed(2);
        assert!(m.consumed_le_materialized());
        assert!(!m.accounting_violation());
        assert_eq!(m.source_bytes_received(), 1);
        assert_eq!(m.input_bytes_consumed(), 2);
    }

    #[test]
    fn scan_work_is_separate_from_consumption() {
        // Peeking/classifying is work but not consumption; entity-expanded
        // bytes are scan work that never passed through the input buffer.
        let mut m = PushMachine::new();
        m.receive_source(4);
        m.materialize_input(4);
        m.note_scan_work(9);
        m.note_consumed(4);
        m.note_scan_work(1000); // bytes examined from an expanded entity
        assert_eq!(m.input_bytes_consumed(), 4);
        assert_eq!(m.total_scan_work(), 1009);
        assert!(m.consumed_le_materialized());
        assert!(
            !m.accounting_violation(),
            "entity scan work is not materialized bytes"
        );
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
    fn default_is_a_clean_start_with_no_position_authority() {
        // Position (line/column) is owned by the input, not the machine: the
        // machine exposes no position fields at all.
        let m = PushMachine::default();
        assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_START);
        assert_eq!(m.source_bytes_received(), 0);
        assert_eq!(m.input_bytes_materialized(), 0);
        assert_eq!(m.input_bytes_consumed(), 0);
        assert_eq!(m.unread(), 0);
        assert!(!m.accounting_violation());
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
        m.receive_source(5);
        m.materialize_input(5);
        assert_eq!(m.resume(false), PushProgress::Progress);
        m.mark_stopped();
        assert_eq!(m.resume(false), PushProgress::Fatal);
        let mut m2 = PushMachine::new();
        m2.receive_source(5);
        m2.materialize_input(5);
        m2.mark_fatal();
        assert_eq!(m2.resume(false), PushProgress::Fatal);
        // ...and even a terminating call is refused.
        assert_eq!(m2.resume(true), PushProgress::Fatal);
    }

    #[test]
    fn eof_phase_with_nothing_unread_reports_document_complete() {
        let mut m = PushMachine::new();
        m.receive_source(4);
        m.materialize_input(4);
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
        m.receive_source(4);
        m.materialize_input(4);
        assert!(m.start_document_fired());
        assert!(!m.end_document_fired(), "endDocument waits for terminate");
        m.mark_end_document_fired();
        assert!(m.end_document_fired());
    }
}
