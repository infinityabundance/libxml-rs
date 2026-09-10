//! Persistent progressive-push machine — §16.7.8 slice 1 foundation.
//!
//! This module holds the state a push parser must keep ALIVE across
//! `xmlParseChunk` calls. The replay architecture it replaces had no place
//! to put that state, so every call re-parsed the whole accumulated buffer
//! (silent probe + delivery reparse), which is both O(N²) and the source of
//! the four push divergence classes the §16.7.8 differential court froze
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
//! A non-final call that runs out of bytes returns [`PushProgress::NeedMoreInput`]
//! and parks; the next call appends bytes to the SAME machine — same cursor,
//! same phase, same stacks — and resumes. Bytes are examined at most once:
//! [`PushMachine::bytes_examined`] must never exceed the total appended, and
//! a correct driver makes it equal (ratio 1.0), whereas the replay design
//! re-examined the whole prefix per call (~40× at 80 chunks).
//!
//! # Scope of this slice
//!
//! This commit installs the machine's state, phase/progress semantics and
//! instrumentation only. The `parse_chunk` driver still runs the replay
//! path; wiring the machine as the delivery path (starting with the trivial
//! element subset `<a/>` / `<a>x</a>` / `<a><b/></a>`, falling back to
//! replay for everything else) is the next step. Nothing here changes
//! observable behavior yet, which is why it can land without touching the
//! frozen court.

/// Outcome of one `resume()` pass over the bytes currently available.
///
/// The distinction between [`NeedMoreInput`](PushProgress::NeedMoreInput)
/// and [`DocumentComplete`](PushProgress::DocumentComplete)/[`Fatal`](PushProgress::Fatal)
/// is the semantic heart of the progressive parser: an incomplete
/// construct at the end of the available bytes is SUSPENSION (the bytes may
/// arrive in a later chunk), not a truncated document and not an error.
/// Upstream expresses this with `xmlParseTryOrFinish`'s `goto done` on
/// non-final calls (`avail < 1`, `xmlParseLookupGt`/`xmlParseLookupCharData`
/// returning 0, the `!terminate` gates), never by raising.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PushProgress {
    /// Bytes were consumed and events may have fired; call `resume()` again.
    Progress,
    /// The available bytes are exhausted inside an incomplete construct
    /// (non-final call): park and wait for more input.
    NeedMoreInput,
    /// The document reached its end (upstream `XML_PARSER_EPILOG`/`EOF`
    /// after `xmlFinishDocument` on the terminating call, or a clean
    /// document end observed with `terminate != 0`).
    DocumentComplete,
    /// A fatal error was raised (`wellFormed == 0` / `disableSAX != 0`);
    /// later chunks are refused with the recorded `errNo`.
    Fatal,
}

/// Document phase, mirroring upstream `ctxt->instate` (parser.h
/// `xmlParserInputState`). The persistent machine keeps this across calls
/// instead of re-deriving it from a whole-buffer reparse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[allow(dead_code)] // phases are introduced as the driver gains each surface
pub(crate) enum PushPhase {
    /// Before the first bytes are classified (upstream gates on `avail < 4`
    /// here for non-final calls).
    #[default]
    Start,
    XmlDecl,
    Misc,
    Dtd,
    Prolog,
    StartTag,
    Content,
    EndTag,
    Epilog,
    Eof,
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
#[derive(Debug, Default)]
pub(crate) struct PushMachine {
    /// Cursor into the accumulated input: the first byte NOT yet examined.
    /// Every byte left of it has been consumed exactly once.
    cursor: usize,
    /// 1-based line of `cursor` (upstream `input->line`).
    line: usize,
    /// 1-based column of `cursor` (upstream `input->col`).
    column: usize,
    /// Current document phase.
    phase: PushPhase,
    /// Open elements, outermost first — the resume point for content.
    open_elements: Vec<ParkedElement>,
    /// In-scope namespace bindings (prefix, href), outermost first;
    /// `ParkedElement::ns_scope_mark` indexes into it.
    ns_scope: Vec<(Vec<u8>, Vec<u8>)>,
    /// `startDocument` fired exactly once, when the phase leaves `Start`.
    start_document_fired: bool,
    /// `endDocument` fired exactly once — on the terminating call (or a
    /// fatal), never merely because a complete document sits in a non-final
    /// chunk.
    end_document_fired: bool,
    /// A fatal error was raised; later chunks are refused with `errNo`.
    fatal: bool,
    /// `xmlStopParser` was called from a callback: refuse further input.
    stopped: bool,
    /// Bytes the machine has examined (instrumentation for the O(N)
    /// invariant: must never exceed `bytes_appended`).
    bytes_examined: u64,
    /// Bytes appended by the caller over the session.
    bytes_appended: u64,
}

#[allow(dead_code)] // driver wiring lands in the next step
impl PushMachine {
    /// A fresh machine at `XML_PARSER_START` with the cursor at 0.
    pub(crate) fn new() -> Self {
        Self {
            line: 1,
            column: 1,
            ..Self::default()
        }
    }

    /// Append `len` newly received bytes. Zero-length calls (including
    /// `xmlParseChunk(NULL, 0, 0)`) append nothing and must not advance any
    /// state by themselves — they only re-run `resume()` over what is
    /// already buffered (upstream's `xmlParseTryOrFinish` over the same
    /// buffer), which is why the court's `zK` plans exist.
    pub(crate) fn append(&mut self, len: usize) {
        self.bytes_appended = self.bytes_appended.saturating_add(len as u64);
    }

    /// Record that `n` bytes have been examined (consumed) by the scanner.
    pub(crate) fn note_examined(&mut self, n: usize) {
        self.bytes_examined = self.bytes_examined.saturating_add(n as u64);
        self.cursor = self.cursor.saturating_add(n);
    }

    pub(crate) const fn phase(&self) -> PushPhase {
        self.phase
    }

    pub(crate) fn set_phase(&mut self, phase: PushPhase) {
        self.phase = phase;
    }

    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) const fn line(&self) -> usize {
        self.line
    }

    pub(crate) const fn column(&self) -> usize {
        self.column
    }

    pub(crate) const fn bytes_examined(&self) -> u64 {
        self.bytes_examined
    }

    pub(crate) const fn bytes_appended(&self) -> u64 {
        self.bytes_appended
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

    /// Whether a byte at the cursor may even be examined: after a stop or a
    /// fatal the machine must refuse, and a zero-length call must never
    /// fabricate progress.
    pub(crate) fn can_resume(&self) -> bool {
        !self.fatal && !self.stopped && self.cursor < self.bytes_appended as usize
    }

    /// Instrumentation gate: the O(N) invariant — no byte is examined twice.
    pub(crate) fn examined_le_appended(&self) -> bool {
        self.bytes_examined <= self.bytes_appended
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
    fn zero_length_calls_do_not_advance_state() {
        let mut m = PushMachine::new();
        m.append(0);
        assert_eq!(m.bytes_appended(), 0);
        assert_eq!(m.cursor(), 0);
        assert!(!m.can_resume(), "a zero-length call has nothing to examine");
        // A zero-length call after real input re-runs over the same buffer.
        m.append(3);
        assert!(m.can_resume());
        assert_eq!(m.cursor(), 0, "cursor parks where the last pass stopped");
    }

    #[test]
    fn examined_bytes_never_exceed_appended() {
        let mut m = PushMachine::new();
        m.append(10);
        m.note_examined(4);
        assert!(m.examined_le_appended());
        assert_eq!(m.cursor(), 4);
        // A resumed pass continues from the parked cursor, it does not
        // re-examine the first four bytes: total examined grows by the new
        // bytes only.
        m.append(2);
        m.note_examined(2);
        assert_eq!(m.bytes_examined(), 6);
        assert_eq!(m.bytes_appended(), 12);
        assert!(m.examined_le_appended());
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
        assert!(m.can_resume());
        m.mark_stopped();
        assert!(!m.can_resume(), "xmlStopParser refuses later chunks");
        let mut m2 = PushMachine::new();
        m2.append(5);
        m2.mark_fatal();
        assert!(!m2.can_resume());
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
