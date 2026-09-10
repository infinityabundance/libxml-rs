//! Persistent, resumable push driver — §16.7.8 slice 1, steps 5/5b.
//!
//! # What this is
//!
//! The control loop that replaces whole-buffer replay. It owns the *execution
//! model* only: which construct to scan next, whether that construct is even
//! available yet, and when the document has finished. Every XML semantic
//! operation it performs is the SAME code the recursive whole-document parser
//! runs — `XmlParser::parse_element_start`, `close_open_element`, the `sax_*`
//! dispatch family, the tokenizer. This is deliberately not a second XML
//! grammar: `state.rs` does not become the pull parser and `pushdrive.rs` does
//! not become an independently maintained one.
//!
//! It is a structural mirror of upstream's two pieces:
//!
//! ```text
//! xmlParseTryOrFinish(ctxt, terminate)   -> drive_persistent's loop
//!   while (disableSAX == 0):
//!     avail = end - cur; if (avail < 1) goto done;
//!     switch (instate) { START | XML_DECL | MISC/PROLOG/EPILOG |
//!                        START_TAG | CONTENT | END_TAG | EOF }
//!
//! xmlParseChunk's terminate block        -> terminate_document()
//!   instate != EOF && != EPILOG -> TAG_NOT_FINISHED / DOCUMENT_EMPTY
//!   else                        -> xmlParserCheckEOF(DOCUMENT_END)
//!   if (instate != EOF) { instate = EOF; xmlFinishDocument(); }
//! ```
//!
//! Note that `endDocument` is fired by the TERMINATE BLOCK, not by
//! `xmlParseTryOrFinish`. That is the mechanical reason a complete document in
//! a non-final chunk does not finish (divergence class 1), and why a REFEED
//! onto a finished context raises `XML_ERR_DOCUMENT_END` on the terminating
//! call with the offending bytes still unread (class 4).
//!
//! # The one invariant
//!
//! > **A byte this driver has consumed is never scanned again, and an
//! > unconsumed byte is inspected a BOUNDED number of times.**
//!
//! The first half is availability gating: the driver looks ahead for a
//! construct's terminator and only then consumes the construct; an incomplete
//! construct parks with the cursor exactly where it was. No rewind, no
//! re-scan of committed bytes, no `suppress_until`.
//!
//! The second half is [`super::push::ParkedConstruct`] — the candidate's
//! `ctxt->checkIndex` / `ctxt->endCheckState`. Every availability scan RESUMES
//! where the previous one stopped instead of restarting at the top of the
//! pending construct, so one huge start tag (or one huge text run) arriving in
//! many chunks is scanned once overall rather than once per chunk. Without
//! this, "consumed at most once" would NOT imply global O(N): it would be
//! quadratic in the length of a single construct.
//!
//! # Scope of this slice
//!
//! ```text
//! START      -> the raw-source availability gate, then XML_DECL
//! XML_DECL   -> XML declaration (or the default version), startDocument, MISC
//! MISC       -> whitespace, comments, PIs, DOCTYPE, then the root start tag
//! DTD        -> the internal subset, once it is all available
//! START_TAG  -> simple / self-closing / nested start tags
//! CONTENT    -> character data, comments, PIs, CDATA, child start tags, end tags
//! END_TAG    -> end tags
//! EPILOG     -> whitespace, comments, PIs, then the terminating transition to EOF
//! EOF        -> REFEED ("Extra content at the end of the document")
//! ```
//!
//! Entity CHARACTER references (`&#65;`, `&amp;`) and parameter/entity
//! declaration CALLBACKS are handled. A GENERAL entity reference runs
//! upstream's `xmlCtxtParseEntity`: the replacement text is parsed with the
//! entity input pushed onto the tokenizer's stack (SEALED, so a construct can
//! never read into the referencing document) while `inputTab`/`inputNr` expose
//! the entity input as `ctxt->input`, exactly as upstream does, and the
//! `reference` event fires afterwards at the document position. That is the
//! SAX half (`ctxt->node == NULL`); the tree half still fills `ent->children`
//! through [`XmlParser::parse_reference`]. Its availability scan
//! (`lookup_char`) parks the construct until the `;` is available.
//!
//! DOCTYPE is modelled in upstream's TWO phases: the declaration HEAD (which
//! records the root/external identifiers and fires `internalSubset`) is parsed
//! as soon as it is available, then `XML_PARSER_DTD` waits for the WHOLE
//! internal subset — upstream's deliberately non-progressive
//! `xmlParseLookupInternalSubset` — before parsing it and firing
//! `externalSubset`. DTD declaration callbacks are dispatched with the cursor
//! repositioned to their own declaration, because upstream fires them inline.
//!
//! The XML declaration, comments, PIs and CDATA are NOT re-implemented: the
//! driver only performs upstream's availability gate and then runs the SAME
//! tokenizer scan and recorder (`parse_xml_decl`, `sax_comment`, `sax_pi`,
//! `sax_cdata`) that the recursive parser uses, so diagnostics and payloads
//! come from one path.
//!
//! Attributes and NAMESPACES are NOT in that list: they come from reusing
//! `parse_element_start` verbatim, so xmlns declarations, prefixed QNames and
//! ancestor binding resolution all work (the oracle-shadow court exercises
//! `<a p="v"/>` and `<a xmlns:x="urn:u"><x:b/></a>` under every plan).
//!
//! The encoder pathways are implemented, not remainders: a DEFINITE invalid
//! unit raises XML_ERR_INVALID_ENCODING before the grammar runs, and an
//! incomplete unit left by a terminating call is flushed by [`check_eof`],
//! matching `xmlParserCheckEOF` (both pinned by the oracle-shadow court's
//! `shadow-utf16invalid.xml` / `shadow-utf16trunc.xml` pairs).
//!
//! A declaration can REPLACE the materialized representation
//! (`convert_declared_units` rebuilds it from byte zero), so any
//! [`ParkedConstruct`] continuation — whose offsets are into that
//! representation — must be invalidated when the representation changes. An
//! ordinary physical rebase (`shrink_window`) must NOT: the stream is never
//! truncated, so absolute offsets survive it.
//!
//! One class-5 remainder is now IMPLEMENTED rather than documented: exact
//! character-data SEGMENTATION. `scan_char_data` reproduces
//! `xmlParseCharDataInternal` including the accelerated byte-class scan, the
//! CR/LF treatment (a CRLF pair is dropped and its LF opens the NEXT
//! callback), the `]`/`]]>` checks, the `BIG_BUFFER_SIZE` rule and the
//! `xmlParseCharDataComplex` fallback for control and non-ASCII bytes — which
//! is where invalid UTF-8 becomes U+FFFD (reported once per input) and an
//! out-of-range scalar becomes `PCDATA invalid Char value`. A single text node
//! is therefore delivered as exactly the sequence of `characters` events
//! upstream delivers, not as one merged event.
//!
//! # Wiring
//!
//! `helpers::parse_chunk` dispatches an ELIGIBLE context to this driver from
//! its first observable event and never switches engines afterwards (see
//! `helpers::push_persistent_eligible`: a push context with options the driver
//! models). The decision is CONTEXT-LEVEL, not "this document has been simple
//! so far" — a later construct can reveal a feature the driver does not
//! model, and once an event has escaped there is no road back to replay
//! because a replay would re-deliver it. Contexts that request an option the
//! driver does not model stay on the historical replay engine, unchanged.

use crate::abi::types::{
    xmlErrorLevel, xmlParserInputState, XML_ERR_DOCUMENT_EMPTY, XML_ERR_DOCUMENT_END,
    XML_ERR_GT_REQUIRED, XML_ERR_INTERNAL_ERROR, XML_ERR_INVALID_CHAR, XML_ERR_MISPLACED_CDATA_END,
    XML_ERR_OK, XML_ERR_TAG_NAME_MISMATCH, XML_ERR_TAG_NOT_FINISHED, XML_FROM_PARSER,
};
use crate::xml::parser::helpers;
use crate::xml::parser::push::{ParkedConstruct, PushMachine, PushProgress};
use crate::xml::parser::state::{OpenElement, XmlParser};
use crate::xml::parser::tokenizer::XmlToken;
use std::os::raw::c_int;

/// Upstream `XML_PARSER_BIG_BUFFER_SIZE` (parser.c): once this many bytes of
/// character data are available, the push parser stops waiting for a `<`/`&`
/// delimiter and parses what it has. Below it, an absent delimiter parks.
const BIG_BUFFER_SIZE: usize = 300;

/// What ONE driver step did.
///
/// The court uses this to enforce the liveness property the driver must have:
/// an `Advanced` step must have changed observable state (consumed bytes,
/// phase, event count, or element-stack depth). Anything else is a spin and
/// fails the court rather than hanging it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // the liveness court matches on every variant; production uses a subset
pub(crate) enum StepOutcome {
    /// Consumed bytes and/or moved the phase and/or dispatched events.
    Advanced,
    /// The next construct is not fully available yet and `terminate == 0`.
    /// The cursor has NOT moved: the next chunk resumes from the same byte,
    /// and the lookahead continuation survives.
    Parked,
    /// The document finished.
    Complete,
    /// A fatal error was raised (`wellFormed == 0`); stop.
    Fatal,
    /// The next construct is outside this slice's grammar. Reported as a loud
    /// internal error rather than being silently mis-parsed or handed back to
    /// the replay engine.
    Unsupported,
}

// The driver is wired into `xmlParseChunk` for eligible contexts
// (`helpers::push_persistent_chunk`), but the allowance stays: the court and
// the machine's diagnostics (`total_scan_work`, `accounting_violation`,
// `events_dispatched`, ...) reach accessors production never calls, and an
// ineligible context deliberately keeps the replay engine (see
// `push_persistent_eligible`).
#[allow(dead_code)]
impl XmlParser {
    /// Feed `chunk` (raw source) into the persistent base input and run one
    /// driver pass. The persistent analogue of `xmlParseChunk`, minus replay.
    ///
    /// The raw-source byte count is NOT recorded here: the input buffer owns
    /// every byte domain (`source_received`, materialized, consumed) and the
    /// machine adopts them through [`PushMachine::sync_input_totals`].
    pub(crate) fn push_persistent(
        &mut self,
        machine: &mut PushMachine,
        chunk: &[u8],
        terminate: bool,
    ) -> PushProgress {
        // Upstream `xmlParseChunk` returns the recorded errNo immediately once
        // `disableSAX` is set — BEFORE pushing, and without dispatching
        // anything again. Without this guard a latched encoder error would be
        // re-raised on every later call (the oracle emits it once).
        if machine.is_fatal()
            || machine.is_stopped()
            || unsafe { (*self.ctxt_raw()).disableSAX } != 0
        {
            machine.mark_fatal();
            return PushProgress::Fatal;
        }

        let pos_before = self.base_input().pos().2 as u64;
        self.base_input_mut().push_bytes_ex(chunk, terminate);
        // A REPLACEMENT of the materialized representation (a declaration-driven
        // transcode or an encoding switch rebuilds it from byte zero) moves the
        // absolute cursor BACKWARDS, and every `ParkedConstruct` offset indexes
        // that representation, so the continuation is meaningless afterwards. An
        // ordinary physical rebase (`shrink_window`) never moves the cursor, and
        // appends only move it forward — both leave the continuation valid, which
        // `parked_construct_survives_a_physical_rebase` pins.
        if (self.base_input().pos().2 as u64) < pos_before {
            machine.reset_construct();
        }
        // `push_bytes_ex` may have REALLOCATED the materialized Vec, so the
        // C-visible window must be re-published before anything observable —
        // including the encoder error raised just below.
        unsafe { self.publish_input_window() };

        // Upstream `xmlParseChunk`: a DEFINITE invalid encoding unit makes
        // `xmlParserInputBufferPush` fail, and `xmlParseChunk` reports
        // XML_ERR_INVALID_ENCODING (via `xmlCtxtErrIO`) BEFORE
        // `xmlParseTryOrFinish` runs. So this call's newly decoded content
        // must not be parsed and no grammar event may fire.
        //
        // The state is NOT left untouched, though: the encoding switch that
        // precedes the parse loop has already happened, and upstream's
        // `xmlDetectEncoding` is what moves `START` to `XML_DECL`
        // (`shadow-utf16invalid.xml` at b257 shows `in=17`, not `in=0`). The
        // position is the input's at the START of the call — the oracle
        // reports `col=1` for a one-chunk feed and `col=4` for a feed that had
        // already consumed `<a>` — which is exactly `InputBuffer::pos()` here,
        // because nothing has been consumed in this call yet.
        if self.base_input().source_encoding_error() {
            if machine.phase() == xmlParserInputState::XML_PARSER_START {
                self.set_phase(machine, xmlParserInputState::XML_PARSER_XML_DECL);
            }
            let (line, col, _) = self.base_input().pos();
            unsafe {
                helpers::raise_invalid_encoding(self.ctxt_raw(), line as c_int, col as c_int);
            }
            machine.mark_fatal();
            return PushProgress::Fatal;
        }

        let outcome = self.drive_persistent(machine, terminate);
        // Refresh before returning control to the caller.
        unsafe { self.publish_input_window() };
        outcome
    }

    /// Run the equivalent of `xmlParseTryOrFinish` to exhaustion, then the
    /// equivalent of `xmlParseChunk`'s terminate block, and report the
    /// machine-level outcome.
    pub(crate) fn drive_persistent(
        &mut self,
        machine: &mut PushMachine,
        terminate: bool,
    ) -> PushProgress {
        // ── xmlParseTryOrFinish ───────────────────────────────────────────
        // Upstream's first act is the buffer shrink:
        //   if ((ctxt->input != NULL) && (ctxt->input->cur - ctxt->input->base > 4096))
        //       xmlParserShrink(ctxt);
        // It is modelled here so the ABI-visible `cur - base` follows the same
        // trajectory. It moves WINDOW METADATA only: the absolute cursor, the
        // decoder state and every absolute offset (`ParkedConstruct::checked`)
        // are untouched, because the Rust stream is never truncated.
        self.base_input_mut().shrink_window();
        unsafe { self.publish_input_window() };
        loop {
            self.sync_accounting(machine);
            if machine.is_fatal() || machine.is_stopped() || self.sax_disabled() {
                machine.mark_fatal();
                return PushProgress::Fatal;
            }
            // `case XML_PARSER_EOF: goto done`
            if machine.phase() == xmlParserInputState::XML_PARSER_EOF {
                break;
            }
            // `avail = end - cur; if (avail < 1) goto done`
            if self.push_remaining_len() == 0 {
                break;
            }
            // Refresh before the step: any callback it dispatches must observe
            // the state at the point it was invoked.
            unsafe { self.publish_input_window() };
            let consumed_before = machine.input_bytes_consumed();
            match self.drive_step(machine, terminate) {
                StepOutcome::Advanced => {
                    self.sync_accounting(machine);
                    let advanced = machine.input_bytes_consumed();
                    if advanced > consumed_before {
                        machine.note_scan_work((advanced - consumed_before) as usize);
                        // The cursor moved, so a lookahead continuation into
                        // the old position is meaningless (upstream clears
                        // checkIndex when the construct is consumed).
                        machine.reset_construct();
                    }
                }
                StepOutcome::Parked | StepOutcome::Complete => break,
                StepOutcome::Fatal | StepOutcome::Unsupported => {
                    machine.mark_fatal();
                    break;
                }
            }
        }

        // ── xmlParseChunk's terminate block ───────────────────────────────
        if terminate && !machine.is_fatal() {
            self.terminate_document(machine);
        }

        if machine.is_fatal() {
            return PushProgress::Fatal;
        }
        if machine.phase() == xmlParserInputState::XML_PARSER_EOF {
            return if machine.unread() == 0 {
                PushProgress::DocumentComplete
            } else {
                // Only reachable on a non-terminating call: a finished context
                // handed more bytes consumes nothing and raises nothing yet
                // (upstream's `case XML_PARSER_EOF: goto done`), so a driver
                // must park here rather than loop.
                PushProgress::AwaitTermination
            };
        }
        PushProgress::NeedMoreInput
    }

    /// One `xmlParseTryOrFinish` switch iteration.
    pub(crate) fn drive_step(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        match machine.phase() {
            xmlParserInputState::XML_PARSER_START => self.step_start(machine),
            xmlParserInputState::XML_PARSER_XML_DECL => self.step_xml_decl(machine, terminate),
            xmlParserInputState::XML_PARSER_MISC
            | xmlParserInputState::XML_PARSER_PROLOG
            | xmlParserInputState::XML_PARSER_EPILOG => self.step_misc(machine, terminate),
            xmlParserInputState::XML_PARSER_START_TAG => self.step_start_tag(machine, terminate),
            xmlParserInputState::XML_PARSER_CONTENT => self.step_content(machine, terminate),
            xmlParserInputState::XML_PARSER_END_TAG => self.step_end_tag(machine, terminate),
            xmlParserInputState::XML_PARSER_DTD => self.step_doctype_subset(machine, terminate),
            xmlParserInputState::XML_PARSER_EOF => StepOutcome::Complete,
            _ => self.unsupported(machine, "parser phase"),
        }
    }

    /// `xmlParseChunk`'s termination block.
    fn terminate_document(&mut self, machine: &mut PushMachine) {
        let phase = machine.phase();
        if phase != xmlParserInputState::XML_PARSER_EOF
            && phase != xmlParserInputState::XML_PARSER_EPILOG
        {
            if !machine.open_elements().is_empty() {
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
                machine.mark_fatal();
            } else if phase == xmlParserInputState::XML_PARSER_START {
                // `xmlFatalErr(ctxt, XML_ERR_DOCUMENT_EMPTY, NULL)`
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
                machine.mark_fatal();
            } else {
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_DOCUMENT_EMPTY,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Start tag expected, '<' not found\n".to_string(),
                    None,
                    None,
                    None,
                    0,
                );
                machine.mark_fatal();
            }
        } else {
            self.check_eof(machine, XML_ERR_DOCUMENT_END);
        }
        if machine.phase() != xmlParserInputState::XML_PARSER_EOF {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
        }
    }

    /// Upstream `xmlParserCheckEOF` (parserInternals.c): raise `code` only when
    /// the context is still error-free AND the input was not consumed
    /// completely. The unconsumed bytes are the evidence — they are never
    /// consumed to make the check succeed (divergence class 4's REFEED).
    ///
    /// Then the ENCODER FLUSH: when the input was consumed completely but the
    /// decoder still holds an incomplete unit, the terminating call must
    /// report XML_ERR_INVALID_ENCODING. This is the half the shadow court's
    /// `shadow-utf16trunc.xml` cell pins down (`error dom=8 code=81`, then
    /// `endDocument`, then `rc=81`).
    fn check_eof(&mut self, machine: &mut PushMachine, code: c_int) {
        if unsafe { (*self.ctxt_raw()).errNo } != XML_ERR_OK {
            return;
        }
        if self.push_remaining_len() > 0 {
            self.raise_error_now(
                XML_FROM_PARSER,
                code,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Extra content at the end of the document\n".to_string(),
                None,
                None,
                None,
                0,
            );
            machine.mark_fatal();
            return;
        }
        if self.base_input().source_truncated() {
            // At EOF every materialized byte has been consumed, so the input's
            // position here IS the stream end the oracle reports.
            let (line, col, _) = self.base_input().pos();
            unsafe {
                helpers::raise_invalid_encoding(self.ctxt_raw(), line as c_int, col as c_int);
            }
            machine.mark_fatal();
        }
    }

    // ── xmlParseTryOrFinish states ─────────────────────────────────────────

    /// `case XML_PARSER_START`.
    ///
    /// The `avail < 4` gate lives in the INPUT layer, not here. Upstream tests
    /// it BEFORE `xmlDetectEncoding`, on RAW source bytes; re-testing it over
    /// MATERIALIZED bytes would be wrong for UTF-16/UCS-4, where four source
    /// bytes materialize as one or two bytes (`FF FE 3C 00` -> `<`). The input
    /// buffer already parks its decoder until it has enough raw evidence
    /// (4 bytes; 200 for the EBCDIC signature), so "still parked" IS the gate.
    fn step_start(&mut self, machine: &mut PushMachine) -> StepOutcome {
        if self.base_input().source_parked() {
            return StepOutcome::Parked;
        }
        self.set_phase(machine, xmlParserInputState::XML_PARSER_XML_DECL);
        StepOutcome::Advanced
    }

    /// `case XML_PARSER_XML_DECL` — the declaration (or its absence), the
    /// default version, then `startDocument`.
    ///
    /// The upstream shape matters: the case LOOKS AHEAD for `<?xml` but only
    /// CONSUMES a real declaration; a `<?...?>` that is not a declaration is
    /// left in place and parsed later, in MISC. Both the declaration scan and
    /// the recorder reuse the recursive parser's code (`scan_pi_or_xml_decl`
    /// and `parse_xml_decl`), so version/encoding/standalone and every
    /// declaration diagnostic come from the same path.
    fn step_xml_decl(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        if !terminate && self.push_remaining_len() < 2 {
            return StepOutcome::Parked;
        }
        if self.push_byte_at(0) == b'<' && self.push_byte_at(1) == b'?' {
            // `if ((!terminate) && (!xmlParseLookupString(ctxt, 2, "?>", 2)))
            //     goto done;`
            if !terminate && !self.lookup_string(machine, 2, b"?>") {
                return StepOutcome::Parked;
            }
            if self.push_slice_at(2, 3) == b"xml" && is_xml_blank(self.push_byte_at(5)) {
                // `ret += 5; xmlParseXMLDecl(ctxt);` — the tokenizer records
                // the declaration and any diagnostic; `parse_xml_decl` only
                // stores version/encoding/standalone on the context.
                self.tokenizer().set_silent_truncated(!terminate);
                let token = self.tokenizer().next_token_raw();
                self.tokenizer().set_silent_truncated(false);
                // The tokenizer QUEUES its diagnostics; the recursive parser
                // flushes them right after each scan, and so must the driver —
                // a queued fatal sets `disableSAX`, which is what suppresses
                // the `startDocument` below (upstream's XML_DECL arm). The
                // flush also leaves `input->cur` at the end of the declaration,
                // which is where the oracle's `startDocument` callback sits.
                self.flush_push_errors();
                let XmlToken::XmlDecl {
                    version,
                    encoding,
                    standalone,
                } = token
                else {
                    return self.unsupported(machine, "XML declaration");
                };
                if self.parse_xml_decl(version, encoding, standalone).is_err() {
                    machine.mark_fatal();
                }
            } else {
                // Not a declaration: upstream sets the default version WITHOUT
                // consuming anything, so the `<?...?>` is parsed as an
                // ordinary PI in MISC.
                self.set_default_version();
            }
        } else {
            self.set_default_version();
        }
        // `if (sax->startDocument && !disableSAX) startDocument()`; the gate
        // lives in `sax_start_document`. The window is re-published first: the
        // declaration scan advanced the cursor, and upstream's callback sees
        // `input->cur` at that point.
        unsafe { self.publish_input_window() };
        if !machine.start_document_fired() {
            if !self.sax_blocked() {
                self.sax_start_document();
                machine.note_event();
            }
            machine.mark_start_document_fired();
        }
        // Upstream assigns MISC unconditionally — even after a fatal
        // declaration error — and the `while (disableSAX == 0)` loop
        // condition then exits the pass.
        self.set_phase(machine, xmlParserInputState::XML_PARSER_MISC);
        if machine.is_fatal() || self.sax_disabled() {
            machine.mark_fatal();
            return StepOutcome::Fatal;
        }
        StepOutcome::Advanced
    }

    /// `while (ctxt->disableSAX == 0)`: any latched fatal (or `xmlStopParser`)
    /// ends the pass. Tokenizer-recorded fatals do not go through the driver's
    /// own error helper, so the loop must consult the context directly.
    fn sax_disabled(&self) -> bool {
        unsafe { (*self.ctxt_raw()).disableSAX != 0 }
    }

    /// `XML_PARSER_MISC`'s DOCTYPE arm: scan the declaration HEAD, record the
    /// root/external identifiers and fire `internalSubset`, then either enter
    /// `XML_PARSER_DTD` (an internal subset follows) or finish the DOCTYPE and
    /// move to PROLOG. Upstream splits exactly here.
    fn step_doctype_decl(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        self.tokenizer().set_silent_truncated(!terminate);
        let token = self.tokenizer().scan_doctype_decl();
        self.tokenizer().set_silent_truncated(false);
        // The scan advanced the cursor; upstream's `internalSubset` callback
        // reads `input->cur` at that point.
        unsafe { self.publish_input_window() };
        self.flush_push_errors();
        let XmlToken::DocTypeDecl {
            content,
            has_subset,
            ..
        } = token
        else {
            return self.unsupported(machine, "DOCTYPE declaration");
        };
        let Ok((root_name, ext_id, sys_id, _)) = self.parse_doctype_decl(&content) else {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        };
        if has_subset {
            // The subset is parsed in `XML_PARSER_DTD`, on a later step and
            // possibly a later call.
            self.set_phase(machine, xmlParserInputState::XML_PARSER_DTD);
            return StepOutcome::Advanced;
        }
        // No subset: consume the `>` the head stopped at (upstream's
        // `if (RAW == '>') NEXT;`), then report and finish the DOCTYPE.
        if self.push_byte_at(0) == b'>' {
            let _ = self.tokenizer().input_mut().current().read_char();
        }
        unsafe { self.publish_input_window() };
        self.flush_push_errors();
        if self
            .finish_doctype(&root_name, ext_id.as_deref(), sys_id.as_deref())
            .is_err()
        {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        self.set_phase(machine, xmlParserInputState::XML_PARSER_PROLOG);
        StepOutcome::Advanced
    }

    /// `case XML_PARSER_DTD`: the internal subset, once upstream's
    /// non-progressive `xmlParseLookupInternalSubset` says it is all here.
    fn step_doctype_subset(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        if !terminate && !self.lookup_internal_subset(machine) {
            return StepOutcome::Parked;
        }
        // The absolute offset of the subset's `[` (the cursor is still on it),
        // so the DTD declaration callbacks can be positioned at their own
        // declaration the way upstream fires them.
        let subset_abs = self.push_bounds().1 as usize;
        self.tokenizer().set_silent_truncated(!terminate);
        let token = self.tokenizer().scan_doctype_subset();
        self.tokenizer().set_silent_truncated(false);
        unsafe { self.publish_input_window() };
        self.flush_push_errors();
        let XmlToken::DocTypeSubset { content, .. } = token else {
            return self.unsupported(machine, "internal subset");
        };
        if self
            .parse_internal_subset(&content, Some(subset_abs))
            .is_err()
        {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        // The identifiers recorded at the declaration HEAD (upstream passes
        // `ctxt->intSubName, ctxt->extSubSystem, ctxt->extSubURI`).
        let (root_name, ext_id, sys_id) = self.doctype_decl_ids();
        if self
            .finish_doctype(&root_name, ext_id.as_deref(), sys_id.as_deref())
            .is_err()
        {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        self.set_phase(machine, xmlParserInputState::XML_PARSER_PROLOG);
        StepOutcome::Advanced
    }

    /// Scan and dispatch one processing instruction. The availability gate is
    /// the CALLER's (`xmlParseLookupString(ctxt, 2, "?>", 2)`); this runs the
    /// tokenizer's PI scan and the recorder `sax_pi`.
    fn scan_pi(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        self.tokenizer().set_silent_truncated(!terminate);
        let token = self.tokenizer().next_token_raw();
        self.tokenizer().set_silent_truncated(false);
        self.flush_push_errors();
        match token {
            XmlToken::ProcessingInstruction { target, data, .. } => {
                self.sax_pi(&target, &data);
                machine.note_event();
                StepOutcome::Advanced
            }
            _ => self.unsupported(machine, "processing instruction"),
        }
    }

    /// Scan and dispatch one comment (the gate is the caller's
    /// `xmlParseLookupString(ctxt, 4, "-->", 3)`).
    fn scan_comment(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        self.tokenizer().set_silent_truncated(!terminate);
        let token = self.tokenizer().next_token_raw();
        self.tokenizer().set_silent_truncated(false);
        self.flush_push_errors();
        match token {
            XmlToken::Comment { data, .. } => {
                self.sax_comment(&data);
                machine.note_event();
                StepOutcome::Advanced
            }
            _ => self.unsupported(machine, "comment"),
        }
    }

    /// Scan and dispatch one CDATA section (the gate is the caller's
    /// `xmlParseLookupString(ctxt, 9, "]]>", 3)`). Upstream sets
    /// `XML_PARSER_CDATA_SECTION` only for the duration of `xmlParseCDSect`
    /// and restores `XML_PARSER_CONTENT` before the next loop iteration, and
    /// nothing observable reads `instate` from inside the section, so the
    /// driver keeps CONTENT throughout.
    fn scan_cdata(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        self.tokenizer().set_silent_truncated(!terminate);
        let token = self.tokenizer().next_token_raw();
        self.tokenizer().set_silent_truncated(false);
        self.flush_push_errors();
        match token {
            XmlToken::Cdata { data, .. } => {
                self.sax_cdata(&data);
                machine.note_event();
                StepOutcome::Advanced
            }
            _ => self.unsupported(machine, "CDATA section"),
        }
    }

    /// `case XML_PARSER_MISC` / `XML_PARSER_PROLOG` / `XML_PARSER_EPILOG`.
    fn step_misc(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let skipped = self.skip_push_whitespace();
        if skipped > 0 {
            machine.note_scan_work(skipped);
            return StepOutcome::Advanced;
        }
        let avail = self.push_remaining_len();
        if avail == 0 {
            return StepOutcome::Parked;
        }
        if self.push_byte_at(0) == b'<' {
            if !terminate && avail < 2 {
                return StepOutcome::Parked;
            }
            let next = self.push_byte_at(1);
            if next == b'?' {
                if !terminate && !self.lookup_string(machine, 2, b"?>") {
                    return StepOutcome::Parked;
                }
                return self.scan_pi(machine, terminate);
            }
            if next == b'!' {
                if !terminate && avail < 3 {
                    return StepOutcome::Parked;
                }
                if self.push_byte_at(2) == b'-' {
                    if !terminate && avail < 4 {
                        return StepOutcome::Parked;
                    }
                    if self.push_byte_at(3) == b'-' {
                        if !terminate && !self.lookup_string(machine, 4, b"-->") {
                            return StepOutcome::Parked;
                        }
                        return self.scan_comment(machine, terminate);
                    }
                } else if machine.phase() == xmlParserInputState::XML_PARSER_MISC {
                    if !terminate && avail < 9 {
                        return StepOutcome::Parked;
                    }
                    if self.push_slice_at(2, 7) == b"DOCTYPE" {
                        if !terminate && !self.lookup_gt(machine) {
                            return StepOutcome::Parked;
                        }
                        return self.step_doctype_decl(machine, terminate);
                    }
                }
                // Not a construct we recognize: fall through to the tail.
            }
        }
        // The tail. In the epilog anything but Misc is extra content; before
        // the root it is the start-tag state's business (which is where the
        // "Start tag expected" diagnostic lives).
        if machine.phase() == xmlParserInputState::XML_PARSER_EPILOG {
            self.check_eof(machine, XML_ERR_DOCUMENT_END);
            // UPSTREAM-PARITY (xmlParseTryOrFinish's MISC/PROLOG/EPILOG arm):
            // `if (ctxt->instate == XML_PARSER_EPILOG) { if (errNo == OK)
            // xmlFatalErr(DOCUMENT_END); instate = EOF; xmlFinishDocument(); }`
            // — the EOF transition and xmlFinishDocument run REGARDLESS of
            // whether the extra-content error was just raised, so endDocument
            // still fires (its dispatch ignores disableSAX; the two-roots
            // cell).
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
        } else {
            self.set_phase(machine, xmlParserInputState::XML_PARSER_START_TAG);
        }
        StepOutcome::Advanced
    }

    /// `case XML_PARSER_START_TAG`.
    fn step_start_tag(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let avail = self.push_remaining_len();
        if !terminate && avail < 2 {
            return StepOutcome::Parked;
        }
        if self.push_byte_at(0) != b'<' {
            // Upstream: XML_ERR_DOCUMENT_EMPTY ("Start tag expected, '<' not
            // found") — the xmlParseTryOrFinish START_TAG arm's message has NO
            // trailing newline (unlike the EPILOG arm and xmlParseChunk's
            // terminate block, which do). instate = EOF, xmlFinishDocument.
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_DOCUMENT_EMPTY,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "Start tag expected, '<' not found".to_string(),
                None,
                None,
                None,
                0,
            );
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        if !terminate && !self.lookup_gt(machine) {
            return StepOutcome::Parked;
        }
        self.open_start_tag(machine)
    }

    /// `case XML_PARSER_CONTENT`.
    fn step_content(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let avail = self.push_remaining_len();
        let cur = self.push_byte_at(0);
        if cur == b'<' {
            if !terminate && avail < 2 {
                return StepOutcome::Parked;
            }
            let next = self.push_byte_at(1);
            if next == b'/' {
                self.set_phase(machine, xmlParserInputState::XML_PARSER_END_TAG);
                return StepOutcome::Advanced;
            }
            if next == b'?' {
                if !terminate && !self.lookup_string(machine, 2, b"?>") {
                    return StepOutcome::Parked;
                }
                return self.scan_pi(machine, terminate);
            }
            if next == b'!' {
                if !terminate && avail < 3 {
                    return StepOutcome::Parked;
                }
                let third = self.push_byte_at(2);
                if third == b'-' {
                    if !terminate && avail < 4 {
                        return StepOutcome::Parked;
                    }
                    if self.push_byte_at(3) == b'-' {
                        if !terminate && !self.lookup_string(machine, 4, b"-->") {
                            return StepOutcome::Parked;
                        }
                        return self.scan_comment(machine, terminate);
                    }
                } else if third == b'[' {
                    if !terminate && avail < 9 {
                        return StepOutcome::Parked;
                    }
                    if self.push_slice_at(2, 7) == b"[CDATA[" {
                        if !terminate && !self.lookup_string(machine, 9, b"]]>") {
                            return StepOutcome::Parked;
                        }
                        return self.scan_cdata(machine, terminate);
                    }
                }
            }
            // `<` followed by a name character: upstream falls out of the `<`
            // checks and reaches `ctxt->instate = XML_PARSER_START_TAG`.
            self.set_phase(machine, xmlParserInputState::XML_PARSER_START_TAG);
            return StepOutcome::Advanced;
        }
        if cur == b'&' {
            if !terminate && !self.lookup_char(machine, b';') {
                return StepOutcome::Parked;
            }
            // Upstream `xmlParseReference`: the SAME routine the recursive
            // parser calls. It expands a character reference inline and, for a
            // general entity, parses the replacement text through the entity
            // input (which is what makes `input->cur` during its callbacks the
            // entity's, not the document's).
            self.tokenizer().set_silent_truncated(!terminate);
            let token = self.tokenizer().next_token_raw();
            self.tokenizer().set_silent_truncated(false);
            self.flush_push_errors();
            let XmlToken::Reference(data) = token else {
                return self.unsupported(machine, "entity/character reference");
            };
            if self.parse_reference(&data).is_err() {
                // UPSTREAM-PARITY (xmlParseTryOrFinish, XML_PARSER_CONTENT's
                // `&` arm): `xmlParseReference(ctxt); break;` — a fatal inside
                // the reference leaves `instate` UNCHANGED and does NOT call
                // xmlFinishDocument. The `while (disableSAX == 0)` loop then
                // exits and xmlParseChunk returns errNo at its
                // `errNo != OK && disableSAX != 0` guard, before the terminate
                // block. Setting EOF + finishing here dispatched a spurious
                // endDocument (the oracle-shadow court's
                // shadow-entity-undef cell).
                return StepOutcome::Fatal;
            }
            machine.note_event();
            self.set_phase(machine, xmlParserInputState::XML_PARSER_CONTENT);
            return StepOutcome::Advanced;
        }
        // Character data. Upstream only consults the `<`/`&` lookup while the
        // available run is shorter than XML_PARSER_BIG_BUFFER_SIZE; at or above
        // it, the run is parsed without waiting for a delimiter (and the
        // continuation is cleared either way).
        if avail < BIG_BUFFER_SIZE {
            if !terminate && !self.lookup_char_data(machine) {
                return StepOutcome::Parked;
            }
        }
        machine.reset_construct();
        // Upstream calls `xmlParseCharDataInternal(ctxt, !terminate)`. The scan
        // and its SEGMENTATION are reproduced in full (class 5): the callback
        // boundaries, the CR/LF treatment and the positions are upstream's, not
        // a merged approximation of one text node.
        self.scan_char_data(machine, terminate)
    }

    // ── Character data (npstream `xmlParseCharDataInternal`) ───────────────

    /// Upstream `xmlParseCharDataInternal`: consume one character-data run from
    /// the base input, delivering EXACTLY the `characters` flushes upstream
    /// delivers — a single text node is routinely split across several events
    /// (see the module docs on class-5 segmentation).
    ///
    /// The accelerated byte-class scan is reproduced byte-for-byte, including
    /// the `]`/`]]>` checks, the CR/LF handling and the 300-byte
    /// `xmlParseCharDataComplex` fallback for control and non-ASCII bytes.
    fn scan_char_data(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        let partial = !terminate;
        let (data_len, start) = self.push_bounds();
        let (mut line, mut col, _) = self.base_input().pos();
        let mut i = start;
        // Upstream `ctxt->input->cur`: the start of the not-yet-flushed range.
        let mut flush_from = start;

        loop {
            // ── get_more_space ────────────────────────────────────────────
            loop {
                while self.abs_byte(i, data_len) == b' ' {
                    i += 1;
                    col += 1;
                }
                if self.abs_byte(i, data_len) == b'\n' {
                    while self.abs_byte(i, data_len) == b'\n' {
                        i += 1;
                        line += 1;
                        col = 1;
                    }
                    continue; // goto get_more_space
                }
                break;
            }
            if self.abs_byte(i, data_len) == b'<' {
                self.flush_char_data(machine, flush_from, i, line, col);
                return self.char_data_outcome(machine, start);
            }
            // ── get_more (the accelerated byte-class scan) ────────────────
            let mut force_flush = false;
            loop {
                while test_char_data(self.abs_byte(i, data_len)) {
                    i += 1;
                    col += 1;
                }
                let c = self.abs_byte(i, data_len);
                if c == b'\n' {
                    while self.abs_byte(i, data_len) == b'\n' {
                        i += 1;
                        line += 1;
                        col = 1;
                    }
                    continue; // goto get_more
                }
                if c == b']' {
                    let avail = data_len.saturating_sub(i);
                    if partial && avail < 2 {
                        force_flush = true;
                        break; // goto invoke_callback
                    }
                    if self.abs_byte(i + 1, data_len) == b']' {
                        if partial && avail < 3 {
                            force_flush = true;
                            break; // goto invoke_callback
                        }
                        if self.abs_byte(i + 2, data_len) == b'>' {
                            // Raised mid-run while `ctxt->input->cur` still sits
                            // at the last flush point but `input->col` has
                            // already been advanced to the ']' — the error is
                            // reported at the FIRST ']'.
                            self.commit_input_pos(i, line, col);
                            self.raise_error_now(
                                XML_FROM_PARSER,
                                XML_ERR_MISPLACED_CDATA_END,
                                xmlErrorLevel::XML_ERR_FATAL as c_int,
                                "Sequence ']]>' not allowed in content\n".to_string(),
                                None,
                                None,
                                None,
                                0,
                            );
                        }
                    }
                    i += 1;
                    col += 1;
                    continue; // goto get_more
                }
                break; // invoke_callback
            }
            // ── invoke_callback ───────────────────────────────────────────
            self.flush_char_data(machine, flush_from, i, line, col);
            flush_from = i;
            if self.abs_byte(i, data_len) == b'\r' && self.abs_byte(i + 1, data_len) == b'\n' {
                // A CRLF pair is one line break, consumed whole. Upstream sets
                // `input->cur` to the LF before consuming it, so the LF is the
                // START of the next flushed range (`\r\ntwo` is delivered as
                // `\ntwo`) — the CR is dropped, never delivered.
                flush_from = i + 1;
                i += 2;
                line += 1;
                col = 1;
                continue; // while (...) — restart at get_more_space
            }
            let c = self.abs_byte(i, data_len);
            if c == b'<' || c == b'&' || force_flush {
                self.commit_input_pos(i, line, col);
                return self.char_data_outcome(machine, start);
            }
            if (0x20..=0x7F).contains(&c) || c == 0x09 || c == 0x0A {
                continue; // while (...) — restart at get_more_space
            }
            // Upstream restores `input->line/col` from the last callback capture
            // here. It is a no-op: the captured values are the live ones.
            return self.scan_char_data_complex(machine, partial, data_len, i, line, col, start);
        }
    }

    /// Upstream `xmlParseCharDataComplex`: the fallback for a run that contains
    /// a byte the accelerated class rejects — a C0 control, a lone CR or a
    /// non-ASCII character. It decodes character by character, flushes every
    /// `XML_PARSER_BIG_BUFFER_SIZE` bytes, and is where invalid UTF-8 becomes
    /// U+FFFD (with `XML_ERR_INVALID_ENCODING` reported once per input) and an
    /// out-of-range scalar becomes `PCDATA invalid Char value %d`.
    #[allow(clippy::too_many_arguments)]
    fn scan_char_data_complex(
        &mut self,
        machine: &mut PushMachine,
        partial: bool,
        data_len: u64,
        mut i: u64,
        mut line: usize,
        mut col: usize,
        start: u64,
    ) -> StepOutcome {
        let mut buf: Vec<u8> = Vec::with_capacity(BIG_BUFFER_SIZE + 4);
        let mut val: u32;
        let mut dec: CharAt;
        loop {
            dec = self.current_char_at(i, data_len, line, col);
            val = dec.val;
            if !(val != u32::from(b'<') && val != u32::from(b'&') && is_xml_char(val)) {
                break;
            }
            if val == u32::from(b']') {
                let avail = data_len.saturating_sub(i);
                if partial && avail < 2 {
                    break;
                }
                if self.abs_byte(i + 1, data_len) == b']' {
                    if partial && avail < 3 {
                        break;
                    }
                    if self.abs_byte(i + 2, data_len) == b'>' {
                        self.commit_input_pos(i, line, col);
                        self.raise_error_now(
                            XML_FROM_PARSER,
                            XML_ERR_MISPLACED_CDATA_END,
                            xmlErrorLevel::XML_ERR_FATAL as c_int,
                            "Sequence ']]>' not allowed in content\n".to_string(),
                            None,
                            None,
                            None,
                            0,
                        );
                    }
                }
            }
            // COPY_BUF
            push_utf8(&mut buf, val);
            i = dec.next;
            line = dec.next_line;
            col = dec.next_col;
            if buf.len() >= BIG_BUFFER_SIZE {
                let flushed = std::mem::take(&mut buf);
                self.flush_char_data_bytes(machine, &flushed, i, line, col);
            }
        }
        if !buf.is_empty() {
            self.flush_char_data_bytes(machine, &buf, i, line, col);
        }
        if i < data_len {
            if val == 0 && self.abs_byte(i, data_len) != 0 {
                if !partial {
                    // A truncated UTF-8 sequence left by a terminating call.
                    // Upstream `xmlFatalErrMsgInt(..., CUR)` reports the leading
                    // byte as BOTH `int1` and the message argument, then runs
                    // `NEXTL(1)` — the offending byte IS consumed.
                    let lead = self.abs_byte(i, data_len);
                    self.commit_input_pos(i, line, col);
                    self.raise_error_now(
                        XML_FROM_PARSER,
                        XML_ERR_INVALID_CHAR,
                        xmlErrorLevel::XML_ERR_FATAL as c_int,
                        format!("Incomplete UTF-8 sequence starting with {lead:02X}\n"),
                        None,
                        None,
                        None,
                        c_int::from(lead),
                    );
                    let (next, next_line, next_col) = self.nextl(i, 1, line, col);
                    i = next;
                    line = next_line;
                    col = next_col;
                }
            } else if val != u32::from(b'<') && val != u32::from(b'&') && val != u32::from(b']') {
                self.commit_input_pos(i, line, col);
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_INVALID_CHAR,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    format!("PCDATA invalid Char value {val}\n"),
                    None,
                    None,
                    None,
                    val as c_int,
                );
                i = dec.next;
                line = dec.next_line;
                col = dec.next_col;
            }
        }
        self.commit_input_pos(i, line, col);
        self.char_data_outcome(machine, start)
    }

    /// Decode the character at absolute offset `i` exactly as upstream
    /// `xmlCurrentChar` does, reporting the diagnostics it reports inline.
    ///
    /// `next`/`next_line`/`next_col` already fold upstream's `NEXTL(l)` (with
    /// the `cur++` `xmlCurrentChar` itself performs for a CRLF pair), so a
    /// caller only has to assign them. `val == 0` with `next == i` is the
    /// incomplete-sequence case (`len == 0`), and the NUL-at-end case.
    fn current_char_at(&mut self, i: u64, data_len: u64, line: usize, col: usize) -> CharAt {
        let b = self.abs_byte(i, data_len);
        if b < 0x80 {
            if b == b'\r' {
                // `input->cur[1]` is read unconditionally upstream (the buffer
                // carries a NUL sentinel), so a trailing CR is a lone CR.
                let crlf = self.abs_byte(i + 1, data_len) == b'\n';
                let (next, next_line, next_col) =
                    self.nextl(if crlf { i + 1 } else { i }, 1, line, col);
                return CharAt {
                    val: 0x0A,
                    next,
                    next_line,
                    next_col,
                };
            }
            if b == 0 {
                if i >= data_len {
                    // End of buffer: `*len = 0`, no diagnostic.
                    return CharAt {
                        val: 0,
                        next: i,
                        next_line: line,
                        next_col: col,
                    };
                }
                self.commit_input_pos(i, line, col);
                self.raise_error_now(
                    XML_FROM_PARSER,
                    XML_ERR_INVALID_CHAR,
                    xmlErrorLevel::XML_ERR_FATAL as c_int,
                    "Invalid character: Char 0x0 out of allowed range\n\n".to_string(),
                    Some(b"Char 0x0 out of allowed range\n".to_vec()),
                    None,
                    None,
                    0,
                );
                let (next, next_line, next_col) = self.nextl(i, 1, line, col);
                return CharAt {
                    val: 0,
                    next,
                    next_line,
                    next_col,
                };
            }
            let (next, next_line, next_col) = self.nextl(i, 1, line, col);
            return CharAt {
                val: u32::from(b),
                next,
                next_line,
                next_col,
            };
        }
        // Multi-byte UTF-8 (upstream xmlCurrentChar).
        let avail = data_len.saturating_sub(i);
        let b1 = self.abs_byte(i + 1, data_len);
        if avail < 2 || (b1 & 0xC0) != 0x80 {
            if avail < 2 {
                return CharAt {
                    val: 0,
                    next: i,
                    next_line: line,
                    next_col: col,
                };
            }
            return self.encoding_error_char(i, line, col);
        }
        let code: u32;
        let len: u64;
        if b < 0xE0 {
            if b < 0xC2 {
                return self.encoding_error_char(i, line, col);
            }
            code = (u32::from(b & 0x1F) << 6) | u32::from(b1 & 0x3F);
            len = 2;
        } else {
            if avail < 3 {
                return CharAt {
                    val: 0,
                    next: i,
                    next_line: line,
                    next_col: col,
                };
            }
            let b2 = self.abs_byte(i + 2, data_len);
            if (b2 & 0xC0) != 0x80 {
                return self.encoding_error_char(i, line, col);
            }
            if b < 0xF0 {
                code = (u32::from(b & 0x0F) << 12)
                    | (u32::from(b1 & 0x3F) << 6)
                    | u32::from(b2 & 0x3F);
                if code < 0x800 || (0xD800..0xE000).contains(&code) {
                    return self.encoding_error_char(i, line, col);
                }
                len = 3;
            } else {
                if avail < 4 {
                    return CharAt {
                        val: 0,
                        next: i,
                        next_line: line,
                        next_col: col,
                    };
                }
                let b3 = self.abs_byte(i + 3, data_len);
                if (b3 & 0xC0) != 0x80 {
                    return self.encoding_error_char(i, line, col);
                }
                code = (u32::from(b & 0x0F) << 18)
                    | (u32::from(b1 & 0x3F) << 12)
                    | (u32::from(b2 & 0x3F) << 6)
                    | u32::from(b3 & 0x3F);
                if !(0x10000..0x110000).contains(&code) {
                    return self.encoding_error_char(i, line, col);
                }
                len = 4;
            }
        }
        let (next, next_line, next_col) = self.nextl(i, len, line, col);
        CharAt {
            val: code,
            next,
            next_line,
            next_col,
        }
    }

    /// Upstream `xmlCurrentChar`'s `encoding_error` arm: report
    /// `XML_ERR_INVALID_ENCODING` ONCE per input and return the replacement
    /// character (which `xmlCurrentCharRecover` substitutes for
    /// `XML_INVALID_CHAR`).
    fn encoding_error_char(&mut self, i: u64, line: usize, col: usize) -> CharAt {
        self.commit_input_pos(i, line, col);
        if !self.base_input().utf8_error_reported() {
            self.base_input_mut().set_utf8_error_reported();
            let (l, c, _) = self.base_input().pos();
            unsafe {
                helpers::raise_invalid_encoding(self.ctxt_raw(), l as c_int, c as c_int);
            }
        }
        let (next, next_line, next_col) = self.nextl(i, 1, line, col);
        CharAt {
            val: 0xFFFD,
            next,
            next_line,
            next_col,
        }
    }

    /// Upstream `NEXTL(l)`: a line break at the CURRENT byte advances the line
    /// and resets the column, anything else advances the column.
    fn nextl(&self, at: u64, l: u64, line: usize, col: usize) -> (u64, usize, usize) {
        let (data_len, _) = self.push_bounds();
        if self.abs_byte(at, data_len) == b'\n' {
            (at + l, line + 1, 1)
        } else {
            (at + l, line, col + 1)
        }
    }

    /// The byte at an absolute offset, or NUL past the materialized end —
    /// libxml2 keeps a NUL sentinel after the buffer and upstream's character
    /// scanner reads `cur[1]`/`cur[2]` unguarded.
    fn abs_byte(&self, k: u64, data_len: u64) -> u8 {
        if k >= data_len {
            0
        } else {
            self.base_input().raw_range(k as usize, k as usize + 1)[0]
        }
    }

    /// Move the base input's cursor to an exact position (upstream advance
    /// `ctxt->input->cur/line/col`).
    fn commit_input_pos(&mut self, pos: u64, line: usize, col: usize) {
        self.base_input_mut()
            .set_diagnostic_position(pos as usize, line, col);
    }

    /// Flush `[from, to)` of the base input as one `characters` event, moving
    /// the cursor to `to` exactly as upstream's `while (in > input->cur)` loop
    /// does (it advances `cur` BEFORE the callback reads it).
    fn flush_char_data(
        &mut self,
        machine: &mut PushMachine,
        from: u64,
        to: u64,
        line: usize,
        col: usize,
    ) {
        if to > from {
            let bytes = {
                let buf = self.base_input();
                buf.raw_range(from as usize, to as usize).to_vec()
            };
            self.commit_input_pos(to, line, col);
            if self.sax_characters_run(&bytes) {
                machine.note_event();
            }
        } else {
            self.commit_input_pos(to, line, col);
        }
    }

    /// Flush the complex path's own buffer (upstream `COPY_BUF`'s stack
    /// `buf`), with the cursor already at the character after the run.
    fn flush_char_data_bytes(
        &mut self,
        machine: &mut PushMachine,
        bytes: &[u8],
        pos: u64,
        line: usize,
        col: usize,
    ) {
        self.commit_input_pos(pos, line, col);
        if self.sax_characters_run(bytes) {
            machine.note_event();
        }
    }

    /// Nothing consumed and nothing delivered means the scan could not make
    /// progress; upstream reaches `goto done` through the availability lookup,
    /// so the driver parks instead of spinning.
    fn char_data_outcome(&mut self, machine: &mut PushMachine, start: u64) -> StepOutcome {
        self.sync_accounting(machine);
        if machine.input_bytes_consumed() == start {
            StepOutcome::Parked
        } else {
            StepOutcome::Advanced
        }
    }

    /// `case XML_PARSER_END_TAG`.
    fn step_end_tag(&mut self, machine: &mut PushMachine, terminate: bool) -> StepOutcome {
        if !terminate && !self.lookup_char(machine, b'>') {
            return StepOutcome::Parked;
        }
        let (name, unterminated) = match self.tokenizer().next_token_raw() {
            XmlToken::EndTag {
                name, unterminated, ..
            } => (name, unterminated),
            _ => return self.unsupported(machine, "end tag"),
        };
        // End-tag scan diagnostics (e.g. the missing '>' the tokenizer saw, an
        // invalid name) are raised by upstream xmlParseEndTag2 inline; the
        // driver scanned the whole construct, so it flushes them here with the
        // cursor repositioned to each diagnostic.
        self.flush_push_errors();
        let top = machine
            .open_elements()
            .last()
            .map(|e| (e.name.clone(), e.line, e.ns_scope_mark));
        let Some((top_name, top_line, ns_scope_mark)) = top else {
            return self.unsupported(machine, "end tag with no open element");
        };
        if unterminated {
            // Upstream xmlParseEndTag2 reports the missing '>' first
            // (XML_ERR_GT_REQUIRED, message from the error TABLE, newline
            // included) and only then runs the name check.
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_GT_REQUIRED,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                "expected '>'\n".to_string(),
                None,
                None,
                None,
                0,
            );
        }
        if name != top_name {
            // UPSTREAM-PARITY (xmlParseEndTag2): `if (name == NULL) name =
            // BAD_CAST "unparsable";` — a missing end-tag name is reported as
            // the literal "unparsable" in both the message and str2.
            let shown_name: &[u8] = if name.is_empty() {
                b"unparsable"
            } else {
                &name
            };
            // A stray end tag closes the CURRENT element anyway (upstream keeps
            // scanning after the mismatch).
            self.raise_error_now(
                XML_FROM_PARSER,
                XML_ERR_TAG_NAME_MISMATCH,
                xmlErrorLevel::XML_ERR_FATAL as c_int,
                format!(
                    "Opening and ending tag mismatch: {} line {} and {}\n",
                    String::from_utf8_lossy(&top_name),
                    top_line,
                    String::from_utf8_lossy(shown_name)
                ),
                Some(top_name.clone()),
                Some(shown_name.to_vec()),
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
        let phase = if machine.open_elements().is_empty() {
            xmlParserInputState::XML_PARSER_EPILOG
        } else {
            xmlParserInputState::XML_PARSER_CONTENT
        };
        self.set_phase(machine, phase);
        StepOutcome::Advanced
    }

    // ── Construct handling ─────────────────────────────────────────────────

    /// Scan and open one start tag (`<name ...>` / `<name ... />`).
    ///
    /// The heavy lifting is `parse_element_start` — the SAME routine the
    /// recursive parser calls: name-stack push, attribute substitution,
    /// namespace classification, the SAX2 start event, tree construction.
    fn open_start_tag(&mut self, machine: &mut PushMachine) -> StepOutcome {
        // The push caller's start-tag-end diagnostic variant (see the
        // tokenizer's `push_start_tag`).
        self.tokenizer().set_push_start_tag(true);
        let tag_start = self.push_bounds().1 as usize;
        let (name, attributes, attr_end, attr_start, end_pos, empty, unterminated) =
            match self.tokenizer().next_token_raw() {
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
                // A `<!DOCTYPE` in CONTENT reaches the start-tag state (the
                // CONTENT arm only recognises `<!--`, `<![CDATA[`, `<?` and
                // `</`). Upstream xmlParseStartTag2's name scan fails at the
                // '!' with XML_ERR_NAME_REQUIRED and returns NULL, so the arm
                // sets instate = EOF and calls xmlFinishDocument. The raise
                // sits on the '!', one byte past the tag's '<'.
                XmlToken::DocType { .. } => {
                    self.raise_error_at(
                        XML_FROM_PARSER,
                        crate::abi::types::XML_ERR_NAME_REQUIRED,
                        xmlErrorLevel::XML_ERR_FATAL as c_int,
                        "StartTag: invalid element name\n".to_string(),
                        None,
                        None,
                        None,
                        0,
                        tag_start + 1,
                    );
                    self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
                    self.finish_document(machine);
                    return StepOutcome::Fatal;
                }
                _ => return self.unsupported(machine, "start tag"),
            };
        if unterminated {
            // The tokenizer recorded the real diagnostics (a name longer than
            // XML_MAX_NAME_LENGTH, an unquoted/duplicate attribute, a tag cut
            // off by the end of the input). Upstream raises them INLINE during
            // xmlParseStartTag2, so they must be flushed here — before the
            // refusal — or the fatal would be swallowed and the parse would
            // report success (the push name-length-limit test).
            let codes = self.tokenizer().peek_error_codes();
            self.flush_push_errors();
            // Two upstream failures present identically here but end in
            // DIFFERENT states, so they must be told apart by the diagnostic
            // the tokenizer recorded:
            //
            //   XML_ERR_GT_REQUIRED — xmlParseStartTag2 parsed the NAME and
            //     failed on the tag END. It does NOT return NULL: the push
            //     arm falls through to `if (ctxt->nameNr == 0) instate =
            //     XML_PARSER_EPILOG else CONTENT`, and no xmlFinishDocument
            //     runs. xmlParseChunk then returns errNo at its
            //     errNo/disableSAX guard, so no endDocument fires and the
            //     context rests at that phase (the starttag-trunc cell).
            //
            //   anything else (XML_ERR_NAME_REQUIRED, a truncated tag with no
            //     '>' at all) — xmlParseStartTag2 returned NULL, and the arm
            //     sets `instate = XML_PARSER_EOF; xmlFinishDocument(ctxt)`,
            //     so endDocument DOES fire (raw-high-name).
            if codes.contains(&crate::abi::types::XML_ERR_GT_REQUIRED) {
                let phase = if machine.open_elements().is_empty() {
                    xmlParserInputState::XML_PARSER_EPILOG
                } else {
                    xmlParserInputState::XML_PARSER_CONTENT
                };
                self.set_phase(machine, phase);
                return StepOutcome::Fatal;
            }
            self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
            self.finish_document(machine);
            return StepOutcome::Fatal;
        }
        // Diagnostics recorded while scanning the tag (attribute syntax,
        // namespace warnings) are raised by upstream xmlParseStartTag2 BEFORE
        // the start-element event, so flush them before dispatching it.
        self.flush_push_errors();
        match self.parse_element_start(name, attributes, attr_end, attr_start, end_pos, empty) {
            Ok(open) => {
                machine.note_event();
                if open.empty {
                    // `<a/>`: the close sequence runs immediately.
                    self.close_open_element(&open);
                    machine.note_event();
                } else {
                    // UPSTREAM-PARITY (SAX1 push): `xmlParseStartTag` (the SAX1
                    // scanner) pushes the element name with `namePush`, which
                    // fills `nameTab` but NOT `pushTab[].line` — so every
                    // diagnostic that reads the open element's line (the
                    // end-tag mismatch, "Premature end of data in tag ... line
                    // %d") reports 0 for a SAX1 consumer. The SAX2 scanner
                    // calls `nameNsPush(..., line, nbNs)` and reports the real
                    // line.
                    let line = if self.sax2_mode() { open.open_line } else { 0 };
                    machine.push_element(open.name.clone(), line, open.ns_scope_mark);
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
                self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
                self.finish_document(machine);
                StepOutcome::Fatal
            }
        }
    }

    // ── Availability lookups (upstream xmlParseLookup*, with continuation) ──

    /// Upstream `xmlParseLookupGt`: is the tag's `>` present, respecting quoted
    /// attribute values? Resumes at the persisted continuation. Found-only —
    /// the CALLER applies upstream's `!terminate` guard.
    fn lookup_gt(&self, machine: &mut PushMachine) -> bool {
        let (data_len, pos) = self.push_bounds();
        let (mut at, mut quote) = match machine.parked_construct() {
            // Upstream starts at `cur + 1` when it has no continuation: the
            // first byte is the '<' and cannot be the '>'.
            ParkedConstruct::Gt { checked, quote }
                if valid_continuation(checked, pos, data_len) =>
            {
                (checked, quote)
            }
            _ => (pos + 1, 0u8),
        };
        let mut examined = 0usize;
        {
            let rem = self.push_slice_from(at);
            for &b in rem {
                examined += 1;
                at += 1;
                if quote != 0 {
                    if b == quote {
                        quote = 0;
                    }
                } else if b == b'\'' || b == b'"' {
                    quote = b;
                } else if b == b'>' {
                    machine.note_scan_work(examined);
                    machine.reset_construct();
                    return true;
                }
            }
        }
        machine.note_scan_work(examined);
        machine.park_construct(ParkedConstruct::Gt { checked: at, quote });
        false
    }

    /// Upstream `xmlParseLookupCharData`: is a `<` or `&` present?
    /// Upstream `xmlParseLookupInternalSubset`: is the WHOLE internal subset
    /// (through `] S? >`) available? Upstream refuses progressive parsing of the
    /// subset, but the lookup itself still carries a continuation
    /// (`checkIndex` + `endCheckState`) so it resumes rather than restarting,
    /// including a bounded 3-byte rescan that detects `<!--`/`-->` split across
    /// chunks.
    fn lookup_internal_subset(&self, machine: &mut PushMachine) -> bool {
        let (data_len, pos) = self.push_bounds();
        let (mut at, mut state) = match machine.parked_construct() {
            ParkedConstruct::Subset { checked, state }
                if valid_continuation(checked, pos, data_len) =>
            {
                (checked, state)
            }
            // `if (ctxt->checkIndex == 0) cur = ctxt->input->cur + 1;`
            _ => (pos + 1, 0u8),
        };
        let mut start = at;
        let scan_from = at;
        let rem = self.push_slice_from(at);
        let mut i = 0usize;
        let mut found = false;
        while i < rem.len() {
            let cur = rem[i];
            let n1 = rem.get(i + 1).copied().unwrap_or(0);
            let n2 = rem.get(i + 2).copied().unwrap_or(0);
            if state == b'-' {
                if cur == b'-' && n1 == b'-' && n2 == b'>' {
                    state = 0;
                    i += 3;
                    start = scan_from + i as u64;
                    continue;
                }
            } else if state == b']' {
                if cur == b'>' {
                    found = true;
                    break;
                }
                if is_xml_blank(cur) {
                    state = b' ';
                } else if cur != b']' {
                    state = 0;
                    start = scan_from + i as u64;
                    continue;
                }
            } else if state == b' ' {
                if cur == b'>' {
                    found = true;
                    break;
                }
                if !is_xml_blank(cur) {
                    state = 0;
                    start = scan_from + i as u64;
                    continue;
                }
            } else if state != 0 {
                if cur == state {
                    state = 0;
                    start = scan_from + i as u64 + 1;
                }
            } else if cur == b'<' {
                if n1 == b'!' && n2 == b'-' && rem.get(i + 3) == Some(&b'-') {
                    state = b'-';
                    i += 4;
                    start = scan_from + i as u64;
                    continue;
                }
            } else if cur == b'"' || cur == b'\'' || cur == b']' {
                state = cur;
            }
            i += 1;
        }
        at = scan_from + i as u64;
        machine.note_scan_work((at - scan_from) as usize);
        if found {
            machine.reset_construct();
            return true;
        }
        // Rescan the three last characters to detect `<!--` and `-->` split
        // across chunks.
        if state == 0 || state == b'-' {
            if at - start < 3 {
                at = start;
            } else {
                at -= 3;
            }
        }
        machine.park_construct(ParkedConstruct::Subset { checked: at, state });
        false
    }

    fn lookup_char_data(&self, machine: &mut PushMachine) -> bool {
        let (data_len, pos) = self.push_bounds();
        let at = match machine.parked_construct() {
            ParkedConstruct::CharData { checked } if valid_continuation(checked, pos, data_len) => {
                checked
            }
            _ => pos,
        };
        let mut examined = 0usize;
        {
            let rem = self.push_slice_from(at);
            for &b in rem {
                examined += 1;
                if b == b'<' || b == b'&' {
                    machine.note_scan_work(examined);
                    machine.reset_construct();
                    return true;
                }
            }
        }
        machine.note_scan_work(examined);
        machine.park_construct(ParkedConstruct::CharData {
            checked: data_len as u64,
        });
        false
    }

    /// Upstream `xmlParseLookupChar`: is `needle` present as a single
    /// character? Starts one byte in, like upstream.
    fn lookup_char(&self, machine: &mut PushMachine, needle: u8) -> bool {
        let (data_len, pos) = self.push_bounds();
        let at = match machine.parked_construct() {
            ParkedConstruct::Char { needle: n, checked }
                if n == needle && valid_continuation(checked, pos, data_len) =>
            {
                checked
            }
            _ => pos + 1,
        };
        let mut examined = 0usize;
        {
            let rem = self.push_slice_from(at);
            for &b in rem {
                examined += 1;
                if b == needle {
                    machine.note_scan_work(examined);
                    machine.reset_construct();
                    return true;
                }
            }
        }
        machine.note_scan_work(examined);
        machine.park_construct(ParkedConstruct::Char {
            needle,
            checked: data_len as u64,
        });
        false
    }

    /// Upstream `xmlParseLookupString`: is `needle` present? Persists the
    /// continuation with upstream's `needle.len() - 1` byte overlap so a
    /// terminator split across chunks is still found.
    fn lookup_string(&self, machine: &mut PushMachine, start_delta: usize, needle: &[u8]) -> bool {
        let (data_len, pos) = self.push_bounds();
        let mut buf = [0u8; 3];
        let nlen = needle.len().min(3);
        buf[..nlen].copy_from_slice(&needle[..nlen]);
        let at = match machine.parked_construct() {
            ParkedConstruct::String {
                checked,
                needle: n,
                needle_len,
            } if needle_len as usize == nlen
                && n[..nlen] == buf[..nlen]
                && valid_continuation(checked, pos, data_len) =>
            {
                checked
            }
            _ => (pos + start_delta as u64).min(data_len as u64),
        };
        let mut examined = 0usize;
        {
            let rem = self.push_slice_from(at);
            examined = rem.len();
            if find_subslice(rem, needle).is_some() {
                machine.note_scan_work(examined);
                machine.reset_construct();
                return true;
            }
        }
        // Rescan `needle_len - 1` bytes: the terminator may straddle the
        // boundary. Mirrors upstream's `end -= strLen - 1`.
        let keep = nlen.saturating_sub(1) as u64;
        let next = (data_len as u64).saturating_sub(keep).max(at);
        machine.note_scan_work(examined);
        machine.park_construct(ParkedConstruct::String {
            checked: next,
            needle: buf,
            needle_len: nlen as u8,
        });
        false
    }

    // ── Small helpers ──────────────────────────────────────────────────────

    /// `(data_len, pos)` of the base input, in absolute byte offsets.
    fn push_bounds(&self) -> (u64, u64) {
        let buf = self.base_input();
        (buf.len() as u64, buf.pos().2 as u64)
    }

    /// Bytes remaining in the base input (the push stream).
    fn push_remaining(&self) -> &[u8] {
        self.input_stack().current_ref().remaining()
    }

    fn push_remaining_len(&self) -> usize {
        self.push_remaining().len()
    }

    /// The unconsumed bytes from absolute offset `at` (upstream's
    /// `input->cur + checkIndex`).
    fn push_slice_from(&self, at: u64) -> &[u8] {
        let buf = self.base_input();
        let pos = buf.pos().2;
        let len = buf.len();
        let start = (at as usize).clamp(pos, len);
        buf.raw_range(start, len)
    }

    /// Byte at an offset from the current position, or NUL past the end —
    /// libxml2 keeps a NUL sentinel after the buffer, and upstream reads
    /// `cur[1]` on a terminating call with a single byte left.
    fn push_byte_at(&self, i: usize) -> u8 {
        self.push_remaining().get(i).copied().unwrap_or(0)
    }

    /// `len` bytes at an offset from the current position, or an empty slice.
    fn push_slice_at(&self, i: usize, len: usize) -> &[u8] {
        let rem = self.push_remaining();
        if i <= rem.len() {
            &rem[i..(i + len).min(rem.len())]
        } else {
            &[]
        }
    }

    /// Skip XML whitespace in the base input; returns the byte count.
    fn skip_push_whitespace(&mut self) -> usize {
        self.base_input_mut().skip_ascii_whitespace()
    }

    /// Mirror the input buffer's authoritative accounting onto the machine
    /// (see [`PushMachine::sync_input_totals`]).
    fn sync_accounting(&mut self, machine: &mut PushMachine) {
        let buf = self.base_input();
        // `InputBuffer::pos()` is `(line, col, byte_offset)` — the BYTE OFFSET
        // is the third element (the first two are 1-based line/col).
        let source = buf.source_bytes_received();
        let materialized = buf.materialized_bytes();
        let consumed = buf.pos().2 as u64;
        machine.sync_input_totals(source, materialized, consumed);
    }

    fn set_phase(&mut self, machine: &mut PushMachine, phase: xmlParserInputState) {
        machine.set_phase(phase);
        unsafe {
            (*self.ctxt_raw()).instate = phase as c_int;
        }
    }

    /// Fire `endDocument` at most once, exactly as upstream `xmlFinishDocument`
    /// does — including when a fatal error has already set `disableSAX`.
    fn finish_document(&mut self, machine: &mut PushMachine) {
        if !machine.end_document_fired() {
            self.finalize_end_document();
            machine.mark_end_document_fired();
            machine.note_event();
        }
    }

    /// A construct this slice's grammar does not cover. Upstream's own
    /// unknown-state arm is `XML_ERR_INTERNAL_ERROR`; the point of the
    /// no-mid-stream-fallback contract is that such a construct must never be
    /// silently handed back to the replay engine after observable state has
    /// escaped.
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
        self.set_phase(machine, xmlParserInputState::XML_PARSER_EOF);
        self.finish_document(machine);
        StepOutcome::Unsupported
    }
}

/// One decoded character plus the position AFTER consuming it (upstream
/// `xmlCurrentChar` + `NEXTL(l)` folded together).
struct CharAt {
    val: u32,
    next: u64,
    next_line: usize,
    next_col: usize,
}

/// Upstream `test_char_data` (parser.c): the byte class of the ACCELERATED
/// character-data scan. Everything else stops it — `<`, `&`, `]`, CR, LF, the
/// C0 controls and every non-ASCII byte (each of which then takes a dedicated
/// branch).
fn test_char_data(b: u8) -> bool {
    b == b'\t' || (b >= 0x20 && b <= 0x7F && b != b'&' && b != b'<' && b != b']')
}

/// Upstream `IS_CHAR` — the XML `Char` production.
fn is_xml_char(c: u32) -> bool {
    c == 0x9
        || c == 0xA
        || c == 0xD
        || (0x20..=0xD7FF).contains(&c)
        || (0xE000..=0xFFFD).contains(&c)
        || (0x10000..=0x10FFFF).contains(&c)
}

/// Upstream `COPY_BUF`: append the UTF-8 encoding of `c`.
fn push_utf8(buf: &mut Vec<u8>, c: u32) {
    let mut tmp = [0u8; 4];
    if let Some(ch) = char::from_u32(c) {
        buf.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
    }
}

/// A persisted lookahead continuation is only usable while the cursor has not
/// moved past it and it still lies inside the buffer.
fn valid_continuation(checked: u64, pos: u64, data_len: u64) -> bool {
    checked >= pos && checked <= data_len
}

fn is_xml_blank(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n'
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
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
    //    same event count and the same final phase as the reference parse. A
    //    construct split across calls is a suspension, never an error.
    // 2. FORWARD-ONLY — consumption never moves backwards and never restarts
    //    from zero; the "poison the consumed prefix" run proves it, because a
    //    prefix re-read would parse NULs instead of the document. And an
    //    unconsumed construct is INSPECTED a bounded number of times, which
    //    the long-single-construct courts prove.
    // 3. LIVENESS — every driver step either changes observable state or
    //    parks.

    /// Documents of this slice's grammar, with the expected observable event
    /// count (1 startDocument + start/end per element + characters + 1
    /// endDocument). The count is the strongest cheap detector of DUPLICATE
    /// events — a replayed startDocument/endDocument or a re-delivered element
    /// changes it, whatever the chunking.
    const DOCS: &[(&str, &[u8], u64)] = &[
        ("empty-element", b"<a/>", 4),
        ("content-text", b"<a>x</a>", 5),
        ("nested-empty", b"<a><b/></a>", 6),
        ("attribute", b"<a p=\"v\"/>", 4),
        // Attributes and namespaces are NOT special-cased here: they come from
        // reusing `parse_element_start`, so the court proves they survive
        // arbitrary chunking rather than asserting them by fiat.
        ("namespaced", b"<a xmlns:x=\"urn:u\"><x:b/></a>", 6),
        // The lexical constructs of MISC/PROLOG/EPILOG/CONTENT. Each is the
        // recursive parser's own scan + recorder, driven only through the
        // availability gate, so the count proves the construct fires exactly
        // one event and survives every partition.
        ("xml-decl", b"<?xml version=\"1.0\"?><a>x</a>", 5),
        (
            "pi-misc-and-content",
            b"<?pi before?><a><?pi inside?>x</a>",
            7,
        ),
        (
            "comment-misc-content-epilog",
            b"<!--top--><a><!--mid-->x</a><!--tail-->",
            8,
        ),
        ("cdata", b"<a><![CDATA[x<y&z]]></a>", 5),
        // DOCTYPE, both shapes. The event count excludes the DTD declaration
        // callbacks (internalSubset / externalSubset / elementDecl): the driver
        // models them as DTD work, not as parser events, exactly like the
        // recursive parser. The oracle-shadow court compares those callbacks
        // line-for-line.
        ("doctype", b"<!DOCTYPE a><a/>", 4),
        (
            "doctype-subset",
            b"<!DOCTYPE a [<!ELEMENT a EMPTY>]><a/>",
            4,
        ),
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
        /// Insert a zero-length NON-final call before termination.
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
        // The whole document in ONE non-final chunk: the class-1 case where the
        // document is complete but endDocument must be deferred.
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
    /// With `poison`, the already-consumed prefix is overwritten with NUL after
    /// every call: a forward-only driver cannot notice.
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
            // The raw-source count is NOT recorded here: the input buffer owns
            // it (see PushMachine::sync_input_totals).

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
                String::from_utf8_lossy(&doc[..doc.len().min(64)])
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

    /// Shared per-plan equivalence assertions.
    fn assert_partition_equivalent(name: &str, doc: &[u8], expected_events: u64) {
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

    // ── 1. Partition equivalence ───────────────────────────────────────────

    #[test]
    fn persistent_driver_matches_the_recursive_parser_under_every_partition() {
        for &(name, doc, expected_events) in DOCS {
            assert_partition_equivalent(name, doc, expected_events);
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

    /// The raw-source `avail < 4` gate: a non-final call with too few SOURCE
    /// bytes makes NO progress at all — no event, `instate` stays START
    /// (class 1). The gate belongs to the input layer, so it is expressed in
    /// raw bytes rather than materialized ones.
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
            // The fourth source byte lets the parser leave START.
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

    /// A [`ParkedConstruct`] stores an ABSOLUTE byte offset, so an ordinary
    /// physical rebase (`xmlParserShrink`: `used > 4096`) must NOT invalidate
    /// it — the next call has to resume the parked scan at the same absolute
    /// byte, not at a window-relative one.
    ///
    /// The `shadow-win-*` threshold archaeology cannot prove this on its own:
    /// its huge quoted attribute stays parked while `used` is still 0, so no
    /// rebase ever happens WITH a live continuation. This test forces that
    /// ordering deliberately (consume > 4096 bytes, THEN park mid-tag).
    #[test]
    fn parked_construct_survives_a_physical_rebase() {
        let mut first = Vec::new();
        first.extend_from_slice(b"<r>");
        first.extend_from_slice(&[b'x'; 5000]);
        first.extend_from_slice(b"<a b='v'");
        let mut doc = first.clone();
        doc.extend_from_slice(b"/></r>");

        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();

            let _ = p.push_persistent(&mut m, &first, false);
            let checked = match m.parked_construct() {
                ParkedConstruct::Gt { checked, .. } => checked,
                other => panic!("expected a parked Gt continuation, got {other:?}"),
            };
            assert_eq!(p.base_input().window_base_abs(), 0, "no rebase yet");
            assert!(
                checked > 4096,
                "the continuation must sit past the threshold"
            );

            // The next pass opens with the shrink, so the window moves WHILE the
            // continuation is live.
            let _ = p.push_persistent(&mut m, b"/></r>", false);
            let wb = p.base_input().window_base_abs();
            assert!(wb > 0, "the rebase did not happen");
            assert!(
                (wb as u64) < checked,
                "the continuation no longer lies inside the rebased window"
            );
            assert_eq!(
                m.parked_construct(),
                ParkedConstruct::BetweenTokens,
                "the parked tag did not resume from the same absolute offset"
            );

            let _ = p.push_persistent(&mut m, &[], true);
            assert_eq!(m.phase(), xmlParserInputState::XML_PARSER_EOF);
            assert_eq!(dump_doc((*guard.0).myDoc), reference_tree(&doc));
        }
    }

    /// A non-`<` document start is `XML_ERR_DOCUMENT_EMPTY` (4) with the
    /// start-tag diagnostic — NOT an internal error.
    #[test]
    fn non_element_document_start_uses_the_document_empty_error() {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();
            let _ = p.push_persistent(&mut m, b"hello", false);
            assert_eq!(p.push_persistent(&mut m, &[], true), PushProgress::Fatal);
            assert_eq!((*guard.0).errNo, XML_ERR_DOCUMENT_EMPTY);
            assert_eq!((*guard.0).wellFormed, 0);
        }
    }

    /// An unfinished document reports the open element, not "Document is
    /// empty" (the terminate block's `nameNr > 0` branch).
    #[test]
    fn unfinished_element_reports_tag_not_finished() {
        unsafe {
            let guard = CtxtGuard(helpers::create_parser_ctxt());
            let buf = InputBuffer::for_push(&[], None);
            let mut p = XmlParser::new_with_flags(InputStack::new(buf), guard.0, false, false);
            let mut m = PushMachine::new();
            let _ = p.push_persistent(&mut m, b"<a>", false);
            let progress = p.push_persistent(&mut m, &[], true);
            assert_eq!(progress, PushProgress::Fatal);
            assert_eq!((*guard.0).errNo, XML_ERR_TAG_NOT_FINISHED);
            assert!(m.end_document_fired());
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

    // ── 4. Complexity: bounded inspection, not just bounded consumption ────

    /// `<abcdefgh/>` repeated: many SMALL complete constructs.
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

    /// ONE start tag whose quoted attribute value is `n` bytes: a single huge
    /// construct that never completes until the very last chunk.
    fn huge_attribute_doc(n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n + 16);
        v.extend_from_slice(b"<r a=\"");
        v.extend(std::iter::repeat_n(b'x', n));
        v.extend_from_slice(b"\"/>");
        v
    }

    /// ONE uninterrupted character-data run of `n` bytes.
    fn huge_text_doc(n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n + 16);
        v.extend_from_slice(b"<r>");
        v.extend(std::iter::repeat_n(b'x', n));
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
    /// constant as the document grows 4x, for each workload shape. A
    /// whole-prefix or whole-construct restart would make the ratio grow
    /// linearly with size.
    #[test]
    fn scan_work_per_byte_stays_bounded_as_the_document_grows() {
        for (label, build) in [
            ("many-small-tags", element_doc as fn(usize) -> Vec<u8>),
            ("one-huge-tag", huge_attribute_doc),
            ("one-huge-text-run", huge_text_doc),
        ] {
            let mut ratios = Vec::new();
            for kib in [128usize, 512] {
                let doc = build(kib * 1024);
                let (materialized, scan_work, _events, violation) = complexity_run(&doc, 8192);
                assert!(!violation, "accounting violation at {kib} KiB ({label})");
                assert_eq!(
                    materialized,
                    doc.len() as u64,
                    "materialized != document at {kib} KiB ({label})"
                );
                let ratio = scan_work as f64 / materialized as f64;
                assert!(
                    ratio < 6.0,
                    "{label}: scan_work/materialized = {ratio:.3} at {kib} KiB"
                );
                ratios.push(ratio);
            }
            // Bounded means it does not DRIFT with size either. A restart from
            // the top of the pending construct would make the 512 KiB ratio
            // ~4x the 128 KiB one.
            assert!(
                ratios[1] < ratios[0] * 1.5 + 0.1,
                "{label}: scan ratio grew with the document: {ratios:?}"
            );
        }
    }

    /// The long-construct shapes must also be SEMANTICALLY correct under every
    /// partition, not merely cheap. These cross the >= 300-byte character-data
    /// gate, so they exercise the incremental path in both roles.
    #[test]
    fn long_constructs_match_the_recursive_parser_under_every_partition() {
        assert_partition_equivalent("long-attribute", &huge_attribute_doc(4096), 4);
        // The text run is delivered in several `characters` calls (upstream's
        // >= 300-byte rule), but the SAX2 handler merges them into ONE text
        // node, so the tree is identical to the single-chunk reference parse.
        // The event count is deliberately NOT asserted: exact segmentation is
        // divergence class 5, which this slice does not model.
        let reference = reference_tree(&huge_text_doc(4096));
        for plan in plans_for(4096 + 7) {
            let doc = huge_text_doc(4096);
            let r = run(&doc, &plan, false);
            assert_eq!(r.tree, reference, "tree mismatch (long-text/{})", plan.name);
            assert_eq!(r.final_phase, xmlParserInputState::XML_PARSER_EOF);
            assert!(r.end_document_fired);
            assert!(!r.violation);
            assert_eq!(r.consumed, r.materialized);
        }
    }

    /// The delta-inspection property, directly: feeding one gigantic tag in
    /// many chunks must inspect each byte a bounded number of times, so the
    /// total lookahead work must be proportional to the tag length rather than
    /// to its square. Without the checkIndex continuation this fails by orders
    /// of magnitude.
    #[test]
    fn one_huge_tag_is_not_rescanned_from_its_start() {
        let n = 512 * 1024;
        let doc = huge_attribute_doc(n);
        let (materialized, scan_work, _events, violation) = complexity_run(&doc, 4096);
        assert!(!violation);
        assert_eq!(materialized, doc.len() as u64);
        // A restart-from-zero scan would average n/2 inspections per call over
        // n/4096 calls: ~64 GiB of work for this document. A bounded constant
        // is a few bytes per byte.
        assert!(
            scan_work < materialized * 8,
            "huge tag was rescanned: scan_work={scan_work} for {materialized} bytes"
        );
    }

    /// The full 1/2/4/8 MiB curve for the many-small-tags workload. Ignored by
    /// default because it builds ~800k tree nodes at 8 MiB; run with `cargo
    /// test --lib -- --ignored --nocapture scan_work_curve_is_linear`.
    #[test]
    #[ignore = "large: run explicitly to regenerate the complexity receipt"]
    fn scan_work_curve_is_linear() {
        for (label, build) in [
            ("many-small-tags", element_doc as fn(usize) -> Vec<u8>),
            ("one-huge-tag", huge_attribute_doc),
            ("one-huge-text-run", huge_text_doc),
        ] {
            let mut prev: Option<(usize, f64)> = None;
            for mib in [1usize, 2, 4, 8] {
                let doc = build(mib * 1024 * 1024);
                let (materialized, scan_work, events, violation) = complexity_run(&doc, 65536);
                assert!(!violation);
                assert_eq!(materialized, doc.len() as u64);
                let ratio = scan_work as f64 / materialized as f64;
                println!(
                    "pushdrive: {label:18} {mib:>2} MiB  materialized={materialized}  \
                     scan_work={scan_work}  scan/byte={ratio:.4}  events={events}"
                );
                if let Some((pmib, pratio)) = prev {
                    assert!(
                        ratio < pratio * 1.25 + 0.05,
                        "{label}: ratio drifted {pmib} -> {mib} MiB"
                    );
                }
                prev = Some((mib, ratio));
            }
        }
    }
}
