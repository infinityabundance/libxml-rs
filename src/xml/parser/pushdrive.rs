//! Persistent, resumable push driver — §16.7.8 slice 1, step 5.
//!
//! # What this is
//!
//! The control loop that replaces whole-buffer replay. It owns the *execution
//! model* only: it decides which construct to scan next, whether that
//! construct is even available yet, and when the document has finished. Every
//! XML semantic operation it performs is the SAME code the recursive
//! whole-document parser runs — `XmlParser::parse_element_start`,
//! `close_open_element`, the `sax_*` dispatch family, the tokenizer. This is
//! deliberately not a second XML grammar: `state.rs` does not become the pull
//! parser and `pushdrive.rs` does not become an independently maintained one.
//!
//! # The one invariant
//!
//! > **A byte this driver has consumed is never scanned again.**
//!
//! That is what makes the engine O(N) instead of O(N²), and it is why the
//! driver does AVAILABILITY GATING rather than speculative scanning: it looks
//! ahead for a construct's terminator (`>` for a tag, `<`/`&` for character
//! data — upstream's `xmlParseLookupGt` / `xmlParseLookupCharData`) and only
//! then consumes the construct. A construct that is not yet complete parks the
//! driver with the cursor exactly where it was; the next chunk resumes from
//! there. No rewind, no re-scan of committed bytes, no `suppress_until`.
//!
//! The lookahead itself is re-run over the pending (unconsumed) construct on
//! each call, exactly as upstream does. That cost is bounded by the length of
//! the ONE incomplete construct, not by the length of the document.
//!
//! # Scope of this slice
//!
//! Deliberately bounded to the grammar that establishes the mechanism:
//!
//! ```text
//! START -> startDocument        (the avail < 4 gate)
//! simple start tag
//! self-closing root             (<a/>)
//! content text                  (<a>x</a>)
//! nested start tag              (<a><b/></a>)
//! end tag
//! root closure -> EPILOG
//! zero-byte termination -> EOF
//! trailing content / REFEED     ("Extra content at the end of the document")
//! ```
//!
//! Everything else (XML declarations, comments, PIs, CDATA, DOCTYPE,
//! attributes beyond what `parse_element_start` already handles, namespaces,
//! entities, the ≥300-byte character-data availability rule of divergence
//! class 5, the `end_in_lf` CR deferral of class 3) is reported as
//! [`StepOutcome::Unsupported`] — a LOUD fatal, never a silent fallback. There
//! is no mid-document persistent→replay retreat available in this design: see
//! the "No mid-stream fallback" contract in [`super::push`].
//!
//! # Not yet wired into `xmlParseChunk`
//!
//! `helpers::parse_chunk` still replays. A context must be committed to this
//! path BEFORE its first observable event, so the driver is exercised by a
//! dedicated court until the grammar covers a coherent subset; whole contexts
//! then flip over at once.

use crate::abi::types::{
    xmlErrorLevel, xmlParserInputState, XML_ERR_DOCUMENT_EMPTY, XML_ERR_DOCUMENT_END,
    XML_ERR_GT_REQUIRED, XML_ERR_INTERNAL_ERROR, XML_ERR_TAG_NAME_MISMATCH,
    XML_ERR_TAG_NOT_FINISHED, XML_FROM_PARSER,
};
use crate::xml::parser::push::{PushMachine, PushProgress};
use crate::xml::parser::state::{OpenElement, XmlParser};
use crate::xml::parser::tokenizer::XmlToken;
use std::os::raw::c_int;

/// What ONE driver step did.
///
/// The court uses this to enforce the liveness property the driver must have:
/// an `Advanced` step must have changed observable state (consumed bytes,
/// phase, event count, or element-stack depth). Anything else is a spin and
/// fails the court rather than hanging it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // returned to the court; the production caller lands with the wiring step
pub(crate) enum StepOutcome {
    /// Consumed bytes and/or moved the phase and/or dispatched events.
    Advanced,
    /// The next construct is not fully available yet and `terminate == 0`.
    /// The cursor has NOT moved: the next chunk resumes from the same byte.
    Parked,
    /// The document finished (endDocument fired, phase `XML_PARSER_EOF`).
    Complete,
    /// A fatal error was raised (`wellFormed == 0`); stop.
    Fatal,
    /// The next construct is outside this slice's grammar. Reported as a loud
    /// internal error rather than being silently mis-parsed or handed back to
    /// the replay engine.
    Unsupported,
}

// COURT-ONLY until the wiring step: every method below is reached from
// `pushdrive::tests` and from nothing else, because a context must be
// committed to this path BEFORE its first observable event and cannot retreat
// to replay mid-document. `parse_chunk` therefore stays on replay until the
// persistent grammar covers a coherent subset, at which point whole contexts
// flip over and this allowance is removed.
#[allow(dead_code)]
impl XmlParser {
    /// Feed `chunk` (raw source) into the persistent base input and run one
    /// driver pass. The persistent analogue of `xmlParseChunk`, minus replay.
    ///
    /// ```text
    /// source_bytes_received += chunk.len()
    /// base_input.push_bytes_ex(chunk, terminate)
    /// drive_persistent(machine, terminate)
    /// ```
    pub(crate) fn push_persistent(
        &mut self,
        machine: &mut PushMachine,
        chunk: &[u8],
        terminate: bool,
    ) -> PushProgress {
        machine.receive_source(chunk.len());
        self.base_input_mut().push_bytes_ex(chunk, terminate);
        self.drive_persistent(machine, terminate)
    }

    /// Run passes until the driver parks, finishes, or fails.
    ///
    /// `PushProgress` is decided by the machine ([`PushMachine::resume`]); the
    /// driver adds the one case the machine cannot see: a finished document
    /// (phase `XML_PARSER_EOF`) that has been handed MORE bytes. Upstream's
    /// `xmlParseTryOrFinish` does nothing in that state (`case
    /// XML_PARSER_EOF: goto done`), so a terminating call's
    /// `xmlParserCheckEOF` observes `cur < end` and raises
    /// `XML_ERR_DOCUMENT_END` — divergence class 4's REFEED. The offending
    /// bytes stay UNREAD: their unread presence IS the evidence.
    pub(crate) fn drive_persistent(
        &mut self,
        machine: &mut PushMachine,
        terminate: bool,
    ) -> PushProgress {
        loop {
            self.sync_accounting(machine);
            match machine.resume(terminate) {
                PushProgress::Progress => {}
                other => return other,
            }

            if machine.phase() == xmlParserInputState::XML_PARSER_EOF {
                // REFEED onto a finished document.
                if terminate {
                    self.raise_error_now(
                        XML_FROM_PARSER,
                        XML_ERR_DOCUMENT_END,
                        xmlErrorLevel::XML_ERR_FATAL as c_int,
                        "Extra content at the end of the document\n".to_string(),
                        None,
                        None,
                        None,
                        0,
                    );
                    machine.mark_fatal();
                    return PushProgress::Fatal;
                }
                return PushProgress::AwaitTermination;
            }

            let consumed_before = machine.input_bytes_consumed();
            match self.drive_step(machine, terminate) {
                StepOutcome::Advanced => {
                    self.sync_accounting(machine);
                    let delta = machine
                        .input_bytes_consumed()
                        .saturating_sub(consumed_before);
                    machine.note_scan_work(delta as usize);
                }
                StepOutcome::Parked => {
                    return if machine.phase() == xmlParserInputState::XML_PARSER_EOF {
                        PushProgress::AwaitTermination
                    } else {
                        PushProgress::NeedMoreInput
                    };
                }
                StepOutcome::Complete => return PushProgress::DocumentComplete,
                StepOutcome::Fatal | StepOutcome::Unsupported => {
                    machine.mark_fatal();
                    return PushProgress::Fatal;
                }
            }
        }
    }

    /// One driver step: scan and dispatch at most one construct, or park.
    pub(crate) fn drive_step(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        match machine.phase() {
            xmlParserInputState::XML_PARSER_START => self.step_start(machine, terminate),
            xmlParserInputState::XML_PARSER_MISC | xmlParserInputState::XML_PARSER_PROLOG => {
                self.step_prolog(machine, terminate)
            }
            xmlParserInputState::XML_PARSER_START_TAG | xmlParserInputState::XML_PARSER_CONTENT => {
                self.step_content(machine, terminate)
            }
            xmlParserInputState::XML_PARSER_END_TAG => self.step_content(machine, terminate),
            xmlParserInputState::XML_PARSER_EPILOG => self.step_epilog(machine, terminate),
            xmlParserInputState::XML_PARSER_EOF => StepOutcome::Complete,
            _ => self.unsupported(machine, "parser phase"),
        }
    }

    /// `XML_PARSER_START` — upstream `xmlParseTryOrFinish`'s
    /// `if ((!terminate) && (avail < 4)) goto done;`.
    ///
    /// Until enough source bytes exist to decide the encoding, NOTHING is
    /// parsed, no event fires and `instate` stays `START` (divergence class
    /// 1's single-byte-feed behavior). `startDocument` fires exactly when the
    /// parser is allowed to leave this state — never earlier, never per call.
    fn step_start(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let avail = self.push_remaining_len();
        if !terminate && avail < 4 {
            return StepOutcome::Parked;
        }
        if avail == 0 {
            // A terminating call on an empty document.
            self.raise_document_empty();
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        if !machine.start_document_fired() {
            self.sax_start_document();
            machine.note_event();
            machine.mark_start_document_fired();
        }
        self.set_phase(machine, xmlParserInputState::XML_PARSER_MISC);
        StepOutcome::Advanced
    }

    /// Misc* before the root element.
    fn step_prolog(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let skipped = self.skip_push_whitespace();
        if skipped > 0 {
            machine.note_scan_work(skipped);
            return StepOutcome::Advanced;
        }
        let avail = self.push_remaining_len();
        if avail == 0 {
            if !terminate {
                return StepOutcome::Parked;
            }
            self.raise_document_empty();
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        if self.push_starts_with(b"<!") || self.push_starts_with(b"<?") {
            return self.unsupported(machine, "prolog declaration/misc");
        }
        if !self.push_starts_with(b"<") {
            // Document-level non-whitespace data: upstream routes this to the
            // start-tag diagnostics (it is not tokenized as PCDATA first).
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_INTERNAL_ERROR,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Start tag expected, '<' not found\n".to_string(),
                None,
                None,
                None,
                0,
            );
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        if !self.terminator_available(machine, terminate) {
            return StepOutcome::Parked;
        }
        self.open_start_tag(machine, terminate)
    }

    /// Element content: text, child start tags, end tags.
    fn step_content(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let avail = self.push_remaining_len();
        if avail == 0 {
            if !terminate {
                return StepOutcome::Parked;
            }
            self.raise_tag_not_finished(machine);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        if self.push_starts_with(b"</") {
            if !self.terminator_available(machine, terminate) {
                return StepOutcome::Parked;
            }
            return self.close_end_tag(machine);
        }
        if self.push_starts_with(b"<") {
            if self.push_starts_with(b"<!--")
                || self.push_starts_with(b"<!")
                || self.push_starts_with(b"<?")
            {
                return self.unsupported(machine, "content declaration/misc");
            }
            if !self.terminator_available(machine, terminate) {
                return StepOutcome::Parked;
            }
            return self.open_start_tag(machine, terminate);
        }
        if self.push_starts_with(b"&") {
            return self.unsupported(machine, "entity/character reference");
        }
        // Character data: available only up to the next `<`/`&`. With no
        // delimiter in the buffer the run is not dispatchable yet on a
        // non-final call — upstream's `xmlParseLookupCharData` gate. (The
        // >= 300-byte rule that lets a non-final call advance WITHOUT a
        // delimiter is divergence class 5 and is deliberately NOT modeled in
        // this slice; it changes callback SEGMENTATION, not correctness.)
        let (examined, delimiter) = self.lookup_char_data();
        machine.note_scan_work(examined);
        if delimiter.is_none() && !terminate {
            return StepOutcome::Parked;
        }
        match self.tokenizer().next_token_raw() {
            XmlToken::Characters(text) => {
                self.sax_characters_text(&text);
                machine.note_event();
                StepOutcome::Advanced
            }
            _ => self.unsupported(machine, "character data"),
        }
    }

    /// Misc* after the root element, and the terminating transition to EOF.
    fn step_epilog(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let skipped = self.skip_push_whitespace();
        if skipped > 0 {
            machine.note_scan_work(skipped);
            return StepOutcome::Advanced;
        }
        if self.push_remaining_len() == 0 {
            if !terminate {
                // The document is complete but must NOT finish here:
                // endDocument belongs to the terminating call (class 1).
                return StepOutcome::Parked;
            }
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Complete;
        }
        // Anything else after the root is extra content (`xmlParserCheckEOF`).
        self.raise_error_now(
            XML_FROM_PARSER,
            XML_ERR_DOCUMENT_END,
            xmlErrorLevel::XML_ERR_FATAL as c_int,
            "Extra content at the end of the document\n".to_string(),
            None,
            None,
            None,
            0,
        );
        self.finish_document(machine);
        StepOutcome::Fatal
    }

    // ── Construct handling (each consumes exactly one complete construct) ────

    /// Scan and open one start tag (`<name ...>` / `<name ... />`).
    ///
    /// The heavy lifting is `parse_element_start` — the SAME routine the
    /// recursive parser calls: name-stack push, attribute substitution,
    /// namespace classification, the SAX2 start event, tree construction.
    fn open_start_tag(&mut self, machine: &mut PushMachine, _terminate: bool) -> StepOutcome {
        let token = self.tokenizer().next_token_raw();
        let (name, attributes, attr_end, attr_start, end_pos, empty, unterminated) = match token {
            XmlToken::StartTag {
                name,
                attributes,
                attr_end,
                attr_start,
                end_pos,
                empty,
                unterminated,
            } => (
                name,
                attributes,
                attr_end,
                attr_start,
                end_pos,
                empty,
                unterminated,
            ),
            _ => return self.unsupported(machine, "start tag"),
        };
        if unterminated {
            // The tokenizer already recorded the real diagnostics. The
            // availability gate means `>` WAS in the buffer, so this is a
            // genuinely malformed tag, not a chunk-boundary truncation.
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        match self.parse_element_start(name, attributes, attr_end, attr_start, end_pos, empty) {
            Ok(open) => {
                machine.note_event();
                if open.empty {
                    // `<a/>`: the close sequence runs immediately.
                    self.close_open_element(&open);
                    machine.note_event();
                } else {
                    machine.push_element(open.name.clone(), open.open_line, open.ns_scope_mark);
                }
                let phase = if machine.open_elements().is_empty() {
                    xmlParserInputState::XML_PARSER_EPILOG
                } else {
                    xmlParserInputState::XML_PARSER_CONTENT
                };
                self.set_phase(machine, phase);
                StepOutcome::Advanced
            }
            Err(()) => {
                self.finish_document(machine);
                StepOutcome::Fatal
            }
        }
    }

    /// Scan and close one end tag, popping the open-element frame.
    fn close_end_tag(&mut self, machine: &mut PushMachine) -> StepOutcome {
        let (name, unterminated) = match self.tokenizer().next_token_raw() {
            XmlToken::EndTag {
                name, unterminated, ..
            } => (name, unterminated),
            _ => return self.unsupported(machine, "end tag"),
        };
        let top = machine
            .open_elements()
            .last()
            .map(|e| (e.name.clone(), e.line, e.ns_scope_mark));
        let Some((top_name, top_line, ns_scope_mark)) = top else {
            return self.unsupported(machine, "end tag with no open element");
        };
        if unterminated {
            // Upstream xmlParseEndTag2 reports the missing '>' first and only
            // then runs the name check.
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_GT_REQUIRED,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "expected '>'".to_string(),
                None,
                None,
                None,
                0,
            );
        }
        if name != top_name {
            // A stray end tag closes the CURRENT element anyway (upstream
            // keeps scanning after the mismatch).
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_TAG_NAME_MISMATCH,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                format!(
                    "Opening and ending tag mismatch: {} line {} and {}\n",
                    String::from_utf8_lossy(&top_name),
                    top_line,
                    String::from_utf8_lossy(&name)
                ),
                Some(top_name.clone()),
                Some(name.clone()),
                None,
                top_line as c_int,
            );
        }
        let open = OpenElement {
            name: top_name,
            open_line: top_line,
            ns_scope_mark,
            empty: false,
        };
        self.close_open_element(&open);
        machine.note_event();
        machine.pop_element();
        if machine.open_elements().is_empty() {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EPILOG);
        }
        StepOutcome::Advanced
    }

    // ── Availability gating (upstream xmlParseLookup* discipline) ────────────

    /// Whether the next construct's terminator is present.
    ///
    /// The scan is upstream `xmlParseLookupGt`: quote-aware search for the
    /// tag's closing `>`. On a TERMINATING call absence is not a reason to
    /// park — the truncated construct is final and the tokenizer will report
    /// it, so this only gates non-final calls.
    fn terminator_available(&mut self, machine: &mut PushMachine, terminate: bool) -> bool {
        let (examined, found) = self.lookup_gt();
        machine.note_scan_work(examined);
        found || terminate
    }

    /// Position of the next `<`/`&` in the unconsumed input.
    ///
    /// Returns `(bytes_examined, index)`. `index == None` means the run
    /// continues to the end of the available input (no delimiter yet).
    fn lookup_char_data(&self) -> (usize, Option<usize>) {
        let rem = self.push_remaining();
        for (i, &b) in rem.iter().enumerate() {
            if b == b'<' || b == b'&' {
                return (i + 1, Some(i));
            }
        }
        (rem.len(), None)
    }

    /// Upstream `xmlParseLookupGt`: find the tag's closing `>` while skipping
    /// quoted attribute values (a `>` inside a quoted value does not end the
    /// tag). Returns `(bytes_examined, found)`.
    fn lookup_gt(&self) -> (usize, bool) {
        let rem = self.push_remaining();
        let mut quote: u8 = 0;
        for (i, &b) in rem.iter().enumerate() {
            if quote != 0 {
                if b == quote {
                    quote = 0;
                }
            } else if b == b'"' || b == b'\'' {
                quote = b;
            } else if b == b'>' {
                return (i + 1, true);
            }
        }
        (rem.len(), false)
    }

    // ── Small helpers ───────────────────────────────────────────────────────

    /// Bytes remaining in the base input (the push stream).
    fn push_remaining(&self) -> &[u8] {
        self.input_stack().current_ref().remaining()
    }

    fn push_remaining_len(&self) -> usize {
        self.push_remaining().len()
    }

    fn push_starts_with(&self, pat: &[u8]) -> bool {
        self.push_remaining().starts_with(pat)
    }

    /// Skip XML whitespace in the base input; returns the byte count.
    fn skip_push_whitespace(&mut self) -> usize {
        self.base_input_mut().skip_ascii_whitespace()
    }

    /// Mirror the input buffer's authoritative accounting onto the machine.
    ///
    /// The INPUT owns these totals: it alone knows about source decoding and
    /// whole-buffer re-materialization, and about rebasing (the ABI-visible
    /// `cur - base` is rebased by `xmlParserShrink`; the machine keeps only the
    /// absolute stream offset).
    fn sync_accounting(&mut self, machine: &mut PushMachine) {
        let buf = self.base_input();
        let materialized = buf.materialized_bytes();
        // `InputBuffer::pos()` is `(line, col, byte_offset)` — the BYTE OFFSET
        // is the third element (the first two are 1-based line/col).
        let consumed = buf.pos().2 as u64;
        machine.sync_input_totals(materialized, consumed);
    }

    fn set_phase(&mut self, machine: &mut PushMachine, phase: xmlParserInputState) {
        machine.set_phase(phase);
        unsafe {
            (*self.ctxt_raw()).instate = phase as c_int;
        }
    }

    /// Fire `endDocument` at most once, wherever the document ends (success or
    /// fatal) — never merely because a complete document sits in a non-final
    /// chunk.
    fn finish_document(&mut self, machine: &mut PushMachine) {
        if !machine.end_document_fired() {
            self.sax_end_document();
            machine.mark_end_document_fired();
            machine.note_event();
        }
    }

    fn raise_document_empty(&mut self) {
        self.raise_error_now(
            XML_FROM_PARSER,
            XML_ERR_DOCUMENT_EMPTY,
            xmlErrorLevel::XML_ERR_FATAL as c_int,
            "Document is empty\n".to_string(),
            None,
            None,
            None,
            0,
        );
    }

    fn raise_tag_not_finished(&mut self, machine: &PushMachine) {
        let (name, line) = machine
            .open_elements()
            .last()
            .map(|e| (e.name.clone(), e.line))
            .unwrap_or_else(|| (Vec::new(), 0));
        self.raise_error_now(
            XML_FROM_PARSER,
            XML_ERR_TAG_NOT_FINISHED,
            xmlErrorLevel::XML_ERR_FATAL as c_int,
            format!(
                "Premature end of data in tag {} line {}\n",
                String::from_utf8_lossy(&name),
                line
            ),
            Some(name),
            None,
            None,
            line as c_int,
        );
    }

    /// A construct this slice's grammar does not cover. Raised LOUDLY: the
    /// whole point of the no-mid-stream-fallback contract is that an
    /// unsupported construct must never be silently handed back to the replay
    /// engine after observable state has escaped.
    fn unsupported(&mut self, machine: &mut PushMachine, what: &str) -> StepOutcome {
        self.raise_error_now(
            XML_FROM_PARSER,
            XML_ERR_INTERNAL_ERROR,
            xmlErrorLevel::XML_ERR_FATAL as c_int,
            format!("persistent push driver: unsupported {what}\n"),
            None,
            None,
            None,
            0,
        );
        self.finish_document(machine);
        StepOutcome::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::structs::{_xmlDoc, _xmlNode, _xmlParserCtxt};
    use crate::xml::parser::helpers;
    use crate::xml::parser::input::{InputBuffer, InputStack};
    use std::fmt::Write as _;

    // ═══════════════════════════════════════════════════════════════════════
    // Court: the persistent driver vs the recursive whole-document parser
    // ═══════════════════════════════════════════════════════════════════════
    //
    // Three properties, in order of strength:
    //
    // 1. PARTITION EQUIVALENCE — every chunk plan produces the same tree, the
    //    same event count, the same final phase as the reference parse. A
    //    construct split across calls is a suspension, never an error.
    // 2. FORWARD-ONLY — consumption never moves backwards and never restarts
    //    from zero; the "poison the consumed prefix" run proves it, because a
    //    prefix re-read would parse NULs instead of the document.
    // 3. LIVENESS — every driver step either changes observable state or
    //    parks; no step can spin.

    /// Documents of this slice's grammar, with the expected observable event
    /// count (1 startDocument + start/end per element + characters + 1
    /// endDocument). The count is the strongest cheap detector of DUPLICATE
    /// events — a replayed startDocument/endDocument or a re-delivered element
    /// changes it, whatever the chunking.
    const DOCS: &[(&str, &[u8], u64)] = &[
        ("empty-element", b"<a/>", 4),
        ("content-text", b"<a>x</a>", 5),
        ("nested-empty", b"<a><b/></a>", 6),
    ];

    struct CtxtGuard(*mut _xmlParserCtxt);
    impl Drop for CtxtGuard {
        fn drop(&mut self) {
            unsafe { helpers::free_parser_ctxt(self.0) };
        }
    }

    /// One chunk plan: the sequence of chunk sizes, plus how the call stream
    /// is shaped around them.
    struct Plan {
        name: String,
        sizes: Vec<usize>,
        /// The first chunk is handed to the constructor (upstream
        /// `xmlCreatePushParserCtxt(initial)`), not pushed.
        ctor_first: bool,
        /// The last non-empty chunk carries `terminate = 1`, so no separate
        /// empty terminating call follows.
        inline_final: bool,
        /// Insert a zero-length NON-final call before termination (the
        /// `xmlParseChunk(NULL, 0, 0)` shape).
        zero_call: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CallObs {
        consumed: u64,
        phase: xmlParserInputState,
        events: u64,
        depth: usize,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Obs(u64, xmlParserInputState, u64, usize);
    impl Obs {
        fn of(m: &PushMachine) -> Self {
            Obs(
                m.input_bytes_consumed(),
                m.phase(),
                m.events_dispatched(),
                m.open_elements().len(),
            )
        }
    }

    struct CaseResult {
        tree: String,
        calls: Vec<CallObs>,
        final_phase: xmlParserInputState,
        start_document_fired: bool,
        end_document_fired: bool,
        events: u64,
        materialized: u64,
        consumed: u64,
        scan_work: u64,
        violation: bool,
    }

    fn fixed(len: usize, size: usize) -> Vec<usize> {
        let mut v = Vec::new();
        let mut left = len;
        while left > 0 {
            let take = size.min(left);
            v.push(take);
            left -= take;
        }
        v
    }

    /// Deterministic chunk sizes in `1..=7` (a small LCG; no `rand` dep).
    fn random_sizes(len: usize, mut seed: u64) -> Vec<usize> {
        let mut v = Vec::new();
        let mut left = len;
        while left > 0 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let take = ((seed >> 33) as usize % 7 + 1).min(left);
            v.push(take);
            left -= take;
        }
        v
    }

    fn plans_for(len: usize) -> Vec<Plan> {
        let mut v = Vec::new();
        for (name, size) in [("b1", 1usize), ("b2", 2), ("b3", 3), ("b5", 5)] {
            v.push(Plan {
                name: name.to_string(),
                sizes: fixed(len, size),
                ctor_first: true,
                inline_final: false,
                zero_call: true,
            });
        }
        // The whole document in ONE non-final chunk: the class-1 case where
        // the document is complete but endDocument must be deferred.
        v.push(Plan {
            name: "whole-nonfinal".to_string(),
            sizes: vec![len.max(1)],
            ctor_first: false,
            inline_final: false,
            zero_call: false,
        });
        // The terminating call carries the document itself.
        v.push(Plan {
            name: "whole-inline-final".to_string(),
            sizes: vec![len.max(1)],
            ctor_first: false,
            inline_final: true,
            zero_call: false,
        });
        v.push(Plan {
            name: "random".to_string(),
            sizes: random_sizes(len, 0x9E37_79B9_7F4A_7C15),
            ctor_first: true,
            inline_final: false,
            zero_call: true,
        });
        v
    }

    fn record(p: &mut XmlParser, m: &mut PushMachine, calls: &mut Vec<CallObs>) {
        p.sync_accounting(m);
        calls.push(CallObs {
            consumed: m.input_bytes_consumed(),
            phase: m.phase(),
            events: m.events_dispatched(),
            depth: m.open_elements().len(),
        });
    }

    /// Drive one document through one plan with the persistent driver.
    ///
    /// With `poison`, the already-consumed prefix is overwritten with NUL
    /// after every call: a forward-only driver cannot notice.
    fn run(doc: &[u8], plan: &Plan, poison: bool) -> CaseResult {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let ctxt = guard.0;
            assert!(!ctxt.is_null());

            let mut chunks: Vec<(Vec<u8>, bool)> = Vec::new();
            let mut pos = 0usize;
            for &sz in &plan.sizes {
                let end = (pos + sz).min(doc.len());
                chunks.push((doc[pos..end].to_vec(), false));
                pos = end;
            }
            if plan.inline_final {
                if let Some(last) = chunks.iter_mut().rev().find(|c| !c.0.is_empty()) {
                    last.1 = true;
                }
            }
            let (initial, rest) = if plan.ctor_first && !chunks.is_empty() {
                let first = chunks.remove(0);
                (first.0, chunks)
            } else {
                (Vec::new(), chunks)
            };

            let buf = InputBuffer::for_push(&initial, None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), ctxt, false, false);
            let mut m = PushMachine::new();
            let mut calls = Vec::new();
            m.receive_source(initial.len());

            // The constructor's initial chunk is parsed by the FIRST
            // xmlParseChunk call; there is nothing new to push for it.
            if !initial.is_empty() {
                let _ = p.drive_persistent(&mut m, false);
                record(&mut p, &mut m, &mut calls);
                if poison {
                    p.base_input_mut().poison_consumed();
                }
            }
            for (chunk, terminate) in rest {
                let _ = p.push_persistent(&mut m, &chunk, terminate);
                record(&mut p, &mut m, &mut calls);
                if poison {
                    p.base_input_mut().poison_consumed();
                }
            }
            if plan.zero_call {
                let _ = p.push_persistent(&mut m, &[], false);
                record(&mut p, &mut m, &mut calls);
                if poison {
                    p.base_input_mut().poison_consumed();
                }
            }
            if !plan.inline_final {
                let _ = p.push_persistent(&mut m, &[], true);
                record(&mut p, &mut m, &mut calls);
            }

            CaseResult {
                tree: dump_doc((*ctxt).myDoc),
                calls,
                final_phase: m.phase(),
                start_document_fired: m.start_document_fired(),
                end_document_fired: m.end_document_fired(),
                events: m.events_dispatched(),
                materialized: m.input_bytes_materialized(),
                consumed: m.input_bytes_consumed(),
                scan_work: m.total_scan_work(),
                violation: m.accounting_violation(),
            }
        }
    }

    /// The reference: the existing recursive whole-document parse.
    fn reference_tree(doc: &[u8]) -> String {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::from_memory(doc, None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let rc = p.parse_document();
            assert_eq!(
                rc,
                0,
                "reference parse failed for {:?}",
                String::from_utf8_lossy(doc)
            );
            let tree = dump_doc((*guard.0).myDoc);
            drop(p);
            tree
        }
    }

    unsafe fn dump_doc(doc: *mut _xmlDoc) -> String {
        if doc.is_null() {
            return "<no-document>".to_string();
        }
        let mut out = String::new();
        let mut cur = (*doc).children;
        while !cur.is_null() {
            dump_node(cur, 0, &mut out);
            cur = (*cur).next;
        }
        out
    }

    unsafe fn dump_node(node: *mut _xmlNode, depth: usize, out: &mut String) {
        let _ = writeln!(
            out,
            "{}{} name={} content={}",
            "  ".repeat(depth),
            (*node).type_,
            cstr((*node).name),
            cstr((*node).content)
        );
        let mut c = (*node).children;
        while !c.is_null() {
            dump_node(c, depth + 1, out);
            c = (*c).next;
        }
    }

    unsafe fn cstr(p: *const u8) -> String {
        if p.is_null() {
            return "-".to_string();
        }
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf8_lossy(std::slice::from_raw_parts(p, len)).into_owned()
    }

    // ── 1. Partition equivalence ───────────────────────────────────────────

    #[test]
    fn persistent_driver_matches_the_recursive_parser_under_every_partition() {
        for &(name, doc, expected_events) in DOCS {
            let reference = reference_tree(doc);
            for plan in plans_for(doc.len()) {
                let ctx = format!("{name}/{}", plan.name);
                let r = run(doc, &plan, false);
                assert_eq!(r.tree, reference, "tree mismatch ({ctx})");
                assert_eq!(
                    r.final_phase,
                    xmlParserInputState::XML_PARSER_EOF,
                    "final phase ({ctx})"
                );
                assert!(r.start_document_fired, "startDocument fired ({ctx})");
                assert!(r.end_document_fired, "endDocument fired ({ctx})");
                assert_eq!(r.events, expected_events, "event count ({ctx})");
                assert!(!r.violation, "accounting violation ({ctx})");
                assert!(
                    r.scan_work >= r.materialized,
                    "scan work below the consumed byte count ({ctx})"
                );
                assert_eq!(
                    r.consumed, r.materialized,
                    "consumed == materialized at EOF ({ctx})"
                );
                for (i, w) in r.calls.windows(2).enumerate() {
                    assert!(
                        w[1].consumed >= w[0].consumed,
                        "consumption moved backwards at call {i} ({ctx})"
                    );
                }
            }
        }
    }

    /// The document must NOT finish on a non-final call: the whole document in
    /// one non-final chunk still has to wait for termination (divergence class
    /// 1's `endDocument` timing).
    #[test]
    fn end_document_waits_for_the_terminating_call() {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();

            let progress = p.push_persistent(&mut m, b"<a/>", false);
            assert_eq!(progress, PushProgress::NeedMoreInput);
            assert!(
                !m.end_document_fired(),
                "endDocument fired on a non-final call"
            );
            assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_EPILOG);
            assert!(m.start_document_fired());

            let progress = p.push_persistent(&mut m, &[], true);
            assert_eq!(progress, PushProgress::DocumentComplete);
            assert!(m.end_document_fired());
            assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_EOF);
        }
    }

    /// The `avail < 4` gate: a non-final call with too few bytes makes NO
    /// progress at all — no event, `instate` stays START (class 1).
    #[test]
    fn short_non_final_feeds_do_not_leave_start() {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();
            let _ = p.push_persistent(&mut m, b"<", false);
            assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_START);
            assert_eq!(m.events_dispatched(), 0);
            let _ = p.push_persistent(&mut m, b"a/", false);
            assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_START);
            assert_eq!(m.events_dispatched(), 0);
            // The fourth byte lets the parser leave START.
            let _ = p.push_persistent(&mut m, b">", false);
            assert_ne!(m.phase(), xmlParserInputState::XML_PARSER_START);
            assert!(m.start_document_fired());
        }
    }

    /// REFEED onto a finished document: the terminating call reports extra
    /// content, and the offending bytes stay UNREAD (class 4).
    #[test]
    fn refeed_after_eof_reports_extra_content_with_bytes_left_unread() {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();
            let _ = p.push_persistent(&mut m, b"<a/>", false);
            assert_eq!(
                p.push_persistent(&mut m, &[], true),
                PushProgress::DocumentComplete
            );
            let consumed_at_eof = m.input_bytes_consumed();

            // A NON-final refeed parks without consuming anything...
            assert_eq!(
                p.push_persistent(&mut m, b"<a/>", false),
                PushProgress::AwaitTermination
            );
            assert_eq!(m.input_bytes_consumed(), consumed_at_eof);
            // ...and the terminating refeed reports the extra content, with the
            // bytes STILL unread (their unread presence IS the evidence).
            assert_eq!(p.push_persistent(&mut m, &[], true), PushProgress::Fatal);
            assert_eq!(m.input_bytes_consumed(), consumed_at_eof);
            assert_eq!(
                (*guard.0).errNo,
                XML_ERR_DOCUMENT_END,
                "REFEED must raise XML_ERR_DOCUMENT_END"
            );
            assert_eq!((*guard.0).wellFormed, 0);
        }
    }

    // ── 2. Forward-only: the consumed prefix is dead ───────────────────────

    #[test]
    fn a_consumed_prefix_is_never_read_again() {
        for &(name, doc, _) in DOCS {
            for plan in plans_for(doc.len()) {
                let ctx = format!("{name}/{}", plan.name);
                let clean = run(doc, &plan, false);
                let poisoned = run(doc, &plan, true);
                assert_eq!(
                    clean.tree, poisoned.tree,
                    "poisoning changed the tree ({ctx})"
                );
                assert_eq!(
                    clean.calls, poisoned.calls,
                    "poisoning changed the trace ({ctx})"
                );
                assert_eq!(
                    clean.events, poisoned.events,
                    "poisoning changed events ({ctx})"
                );
            }
        }
    }

    // ── 3. Liveness: every step advances or parks ──────────────────────────

    #[test]
    fn every_driver_step_advances_or_parks() {
        for &(name, doc, expected_events) in DOCS {
            unsafe {
                let guard = CtxtGuard(helpers::create_parser_ctxt());
                let buf = InputBuffer::for_push(&[], None);
                let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
                let mut m = PushMachine::new();
                m.receive_source(doc.len());
                p.base_input_mut().push_bytes_ex(doc, false);

                let mut steps = 0usize;
                loop {
                    p.sync_accounting(&mut m);
                    let before = Obs::of(&m);
                    let out = p.drive_step(&mut m, false);
                    p.sync_accounting(&mut m);
                    let after = Obs::of(&m);
                    steps += 1;
                    match out {
                        StepOutcome::Advanced => assert_ne!(
                            before, after,
                            "step {steps} of {name} consumed nothing and changed nothing"
                        ),
                        StepOutcome::Parked => break,
                        other => panic!("unexpected {other:?} at step {steps} of {name}"),
                    }
                    assert!(steps < 64, "driver did not settle for {name}");
                }
                assert_eq!(
                    p.drive_persistent(&mut m, true),
                    PushProgress::DocumentComplete
                );
                assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_EOF);
                // endDocument is part of the count, so assert only after the
                // terminating pass.
                assert_eq!(m.events_dispatched(), expected_events, "events for {name}");
            }
        }
    }

    // ── 4. Complexity: scanning is linear, not quadratic ───────────────────

    /// `<r>` + N * `<abcdefgh/>` + `</r>`: markup-dense so the measurement is
    /// the scanner, and long enough to cross many chunks.
    fn element_doc(bytes: usize) -> Vec<u8> {
        const UNIT: &[u8] = b"<abcdefgh/>";
        let n = (bytes / UNIT.len()).max(1);
        let mut v = Vec::with_capacity(n * UNIT.len() + 16);
        v.extend_from_slice(b"<r>");
        for _ in 0..n {
            v.extend_from_slice(UNIT);
        }
        v.extend_from_slice(b"</r>");
        v
    }

    /// Drive a whole document in fixed chunks and return the complexity
    /// receipt `(materialized, scan_work, events, violation)`.
    fn complexity_run(doc: &[u8], chunk: usize) -> (u64, u64, u64, bool) {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();
            let mut pos = 0usize;
            while pos < doc.len() {
                let end = (pos + chunk).min(doc.len());
                let _ = p.push_persistent(&mut m, &doc[pos..end], false);
                pos = end;
            }
            let _ = p.push_persistent(&mut m, &[], true);
            (
                m.input_bytes_materialized(),
                m.total_scan_work(),
                m.events_dispatched(),
                m.accounting_violation(),
            )
        }
    }

    /// The architectural proof: `scan_work / materialized` stays a bounded
    /// constant as the document grows 4x. A whole-prefix restart would make
    /// the ratio grow linearly with the document size.
    #[test]
    fn scan_work_per_byte_stays_bounded_as_the_document_grows() {
        let mut ratios = Vec::new();
        for kib in [128usize, 256, 512] {
            let doc = element_doc(kib * 1024);
            let (materialized, scan_work, _events, violation) = complexity_run(&doc, 8192);
            assert!(!violation, "accounting violation at {kib} KiB");
            assert_eq!(
                materialized,
                doc.len() as u64,
                "materialized != document at {kib} KiB"
            );
            let ratio = scan_work as f64 / materialized as f64;
            assert!(
                ratio < 6.0,
                "scan_work/materialized = {ratio:.3} at {kib} KiB (a prefix restart would be ~O(size))"
            );
            ratios.push(ratio);
        }
        // Bounded means it does not DRIFT with size either.
        let first = *ratios.first().unwrap();
        let last = *ratios.last().unwrap();
        assert!(
            last < first * 1.5 + 0.1,
            "scan ratio grew with the document: {ratios:?}"
        );
    }

    /// The full 1/2/4/8 MiB curve. Ignored by default because it builds ~800k
    /// tree nodes at 8 MiB; run with `cargo test --lib -- --ignored
    /// --nocapture scan_work_curve_is_linear` to regenerate the receipt.
    #[test]
    #[ignore = "large: run explicitly to regenerate the complexity receipt"]
    fn scan_work_curve_is_linear() {
        let mut prev: Option<(usize, f64, u64)> = None;
        for mib in [1usize, 2, 4, 8] {
            let doc = element_doc(mib * 1024 * 1024);
            let (materialized, scan_work, events, violation) = complexity_run(&doc, 65536);
            assert!(!violation);
            assert_eq!(materialized, doc.len() as u64);
            let ratio = scan_work as f64 / materialized as f64;
            println!(
                "pushdrive: {:>2} MiB  materialized={}  scan_work={}  scan/byte={:.4}  events={}",
                mib, materialized, scan_work, ratio, events
            );
            if let Some((pmib, pratio, _)) = prev {
                assert!(
                    ratio < pratio * 1.25 + 0.05,
                    "ratio drifted {pmib} -> {mib} MiB"
                );
            }
            prev = Some((mib, ratio, scan_work));
        }
    }
}
