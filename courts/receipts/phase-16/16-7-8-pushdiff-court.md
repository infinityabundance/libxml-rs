# Phase 16.7.8 slice 0 — push-parser differential court + baseline divergence inventory

Date: 2026-09-09
Commit: (slice 0: this file's first commit; slice 0.1: court hardening,
rerun on the unchanged parser, corrected baseline below)
Gate: `sh courts/suites/phase16/pushdiff-run.sh` (host) →
`pushdiff-differential.sh` (inside `libxml-rs/phase14-debian:1`)

## What was added

The ground-truth gate for the incremental push parser (16.7.8): a generic
push driver + adversarial corpus + provider-isolated runner that compares
the ORACLE and the CANDIDATE byte-for-byte on every (document, chunking
plan, sax mode) cell:

- `courts/suites/phase16/pushdiff-probe.c` — feeds a document to
  `xmlCreatePushParserCtxt` + `xmlParseChunk` under a chunking plan
  (fixed N-byte, or seeded random splits) with a full recorder SAX table
  (SAX1 and SAX2) + a structured error handler. Prints ONE merged,
  deterministic, flushed-per-record trace: per-call rc/errNo/wellFormed/
  instate interleaved with every event and error in invocation order.
  Recorders copy SAX2 attribute values with the (value, end) convention
  (they are not NUL-terminated). After the document bytes, every plan sends
  one empty terminating call, then one REFEED call on the finished context
  (gh12254's parse-twice surface).
- `courts/suites/phase16/gen_pushdiff_corpus.py` — 136 adversarial docs:
  CR/CRLF/lone-CR line endings (incl. documents and chunk boundaries that
  END in `\r` — the `end_in_lf` deferral case), UTF-8 multibyte content and
  names, entities, DOCTYPE internal subsets with ELEMENT/ATTLIST/ENTITY/
  NOTATION declarations, truncations at every construct (`<a b="`, `<!ELEMENT a (b|c`,
  `<!--`, `<![CDATA[`, `<?t`, `<!DOCTY`, `</`, `&amp`, …), `]]>` in text
  and attributes, mismatched/unclosed roots, epilog junk, comments-only /
  PI-only / empty inputs, namespace forms, raw invalid/truncated UTF-8.
- `courts/suites/phase16/pushdiff-differential.sh` +
  `pushdiff-run.sh` — the in-image gate and the host launcher (court image
  conventions: oracle at `/usr/local`, candidate at `/candidate`).

Corpus = the 136 generated docs + phase-14 fixture XMLs ≤ 32 KiB
(210 documents). Chunking plans are picked by document size so that
single-byte chunking (every byte offset a chunk boundary) is applied to
every document ≤ 9 KiB while larger fixtures use fixed/random plans.
Raw streams: `raw/pushdiff/oracle-*`, `cand-*`, per-cell diffs in
`raw/pushdiff/diffs/`, `summary.txt`.

## Slice 0.1 — court hardening (before freezing the baseline)

The recorder defects below were corrected BEFORE the baseline was frozen;
slice 0.1 reran the UNCHANGED parser and re-measured. The divergence
classes and counts are essentially identical (2619/2624 before and after),
confirming the inventory is structural and did not depend on the bugs.

1. **DTD callback ABI (fixed)**: `entityDeclSAXFunc` is six arguments
   (ctx, name, type, publicId, systemId, content) — the recorder had seven
   with shifted semantics; `attributeDecl` now receives the real
   `xmlEnumerationPtr` and `elementDecl` the real `xmlElementContentPtr`.
   The probe previously declared those as `const xmlChar *` and would have
   run `xmlStrlen` over struct memory (UB).
2. **Canonical recorders**: `xmlElementContent` trees are serialized
   structurally (`{t=<type> o=<ocur> ...}` recursive — never as strings)
   and `xmlEnumeration` chains as `one|two|three`; a wrong-but-non-NULL
   enumeration/content tree now diffs.
3. **Full SAX2 attribute tuples**: every attribute prints all five
   components (localname, prefix, URI, value with the (value, end)
   length convention, end-present flag) in order; namespace resolution
   differences are now visible even when local names/values match.
4. **More of the observable SAX surface**: `notationDecl`,
   `unparsedEntityDecl`, `reference`, `ignorableWhitespace` recorders
   added (SAX1 and SAX2 tables). (SAX1 `attribute` does not exist in
   libxml2 ≥ 2.9's `xmlSAXHandler` and is not registered.)
5. **Deterministic random plans**: the PRNG is seeded ONCE per
   document+plan and advanced per split — previously it was reset every
   iteration, so `rN` degenerated to a fixed-size chunking.
6. **Strict compilation**: the probe builds with `-Wall -Wextra -Werror`
   against each provider's own headers (no `-w` hiding ABI drift).
7. **Per-cell timeout**: every provider cell runs under a bounded
   `timeout`; a timeout is a first-class differential failure
   (TIMEOUT/CRASH), never a hang of the campaign.

## Corrected baseline (unchanged parser, hardened court)

| metric | value |
|---|---:|
| cells (document × plan × sax-mode) | 2624 |
| diverging cells | 2619 |
| event-level divergences (startDocument/startElement/characters/DTD-decl/error records differ between calls) | 2470 |
| REFEED-only divergences (identical feed/events; only the post-finish refeed differs) | 75 |
| other (instate/rc line diffs only) | 74 |

Sanity: same aggregate as the pre-hardening run (2619/2624) — the three
systematic classes below are independent of the corrected recorders.
With the hardened recorders the DTD-declaration cells (elementDecl
content models, attributeDecl enumeration chains, entityDecl) now compare
exactly, and they too diverge on event timing, not on content.

## Slice 0.2 — more lifecycle surfaces; extended baseline (still the
unchanged parser)

New coverage in the court before slice 1 (reviewer items 1–5):

1. **Constructor-initial chunk** (`C<plan>`): the first split goes to
   `xmlCreatePushParserCtxt(chunk, size, …)` and parsing continues via
   `xmlParseChunk` — the API's other lifecycle entrance. Corpus plans
   cover constructor bytes `"<"`, `"<a"`, `"<?xml…"`, complete
   documents, and (via the CR corpus) a trailing `\r`.
2. **`xmlCtxtResetPush` cells** (`Rz<plan>` empty reset then parse B;
   `Ri<plan>` reset with whole B then only the terminating call):
   six shape-different document pairs × 3 plans each, both directions
   (DTD ↔ namespaces, CR line endings ↔ attr-heavy, error doc ↔ CDATA,
   Unicode content) — stale element/namespace/DTD/CR/UTF-8 state must
   not leak across the reset.
3. **Logical-cursor trace on every call** (new invariant): after every
   chunk the trace records input offset (`cur - base`), line, col,
   inputNr and nameNr. The oracle must equal the candidate in parser
   position after EVERY chunk, not merely in emitted events.
4. **Zero-length non-final injections** (`<plan>zK`): K
   `xmlParseChunk(NULL, 0, 0)` calls after every real chunk — a
   suspension must not finish/emit/reset anything.
5. **`xmlStopParser` cells** (`-S K`): stop inside the K-th start-element
   callback; every later chunk must be refused with the recorded error.
6. The receipt nit: single-byte chunking now covers docs ≤ 9216 bytes
   (9 KiB, matching the script).

### Extended baseline (unchanged parser)

| metric | value |
|---|---:|
| cells | 3449 |
| diverging cells | 3449 |
| with event-level diffs | 3380 |
| cursor-field-only diffs (identical event streams; only p/l/col/i/n differ) | 69 |

Adding the cursor invariant moved the result from 2619/2624 to 3449/3449:
no cell is compatible at the level of parser position, not merely events.
Notable new evidence:

- **nameNr inflation confirmed**: in the `-S 2` stop cell the oracle
  rests at `n=2` (its real open stack) while the candidate reports
  `n=6` — every whole-buffer re-parse pushes the already-open names
  again (the candidate's name stack is only balanced across calls when
  the doc completes, and stays stale when it does not).
- The `xmlns: URI … is not absolute` warning fires on a different call
  in the candidate (each re-parse re-raises/re-times it).
- Oracle `p` (cur − base) is bounded by `xmlParserShrink` (> 4096 →
  base advances); the candidate's buffer position accounting must mirror
  that to satisfy the invariant on documents longer than 4 KiB.
- Stop cells: the oracle refuses every post-stop chunk with rc=errNo
  (111); the candidate's first post-stop call reports errNo 111 but
  rc=0 (its disableSAX gate returns after re-parsing).

(Slice 0.3, commit 84156cbd, previously added: suspended-reset cells
`Rs`/`Rsi`, forced constructor-boundary corpus docs + Cb5/Cb12 plans,
the CTOR `ok=` trace form, and the receipt-heading dedup.)

## Slice 0.3.1 — genuine EOF-truncated UTF-8 resets; exact terminology;
forensic provenance chain

Final slice-0 corrections (reviewer), then slice 0 is frozen permanently:

1. **Real pending-UTF-8 reset inputs**: the previous `raw-trunc-utf8.xml`
   (`<a>\xC3</a>`) was NOT a stream ending mid-sequence — the `0xC3` is
   followed by `<`, an invalid continuation byte, so that cell tested
   fatal-invalid-UTF-8→reset (the oracle reaches rc=81 before the reset).
   Added genuinely EOF-truncated sequences the decoder cannot finish:
   `raw-pending-utf8-2.xml` (`<a>\xC3` — 1 continuation byte missing),
   `raw-pending-utf8-3.xml` (`<a>\xE2\x82` — 1 more), `raw-pending-utf8-4.xml`
   (`<a>\xF0\x9F\x8E` — 1 more), plus `raw-pending-entity.xml`
   (`<a>&am` — entity reference name still unterminated). The suspended
   reset pairs now use these; the fatal-UTF-8 docs stay in the corpus for
   the error-class cells.
2. **Exact terminology**: reset cells over syntactically complete A
   documents (fed but never terminated) are labelled **unfinalized-reset**;
   only the genuinely truncated-A cells are labelled
   **suspended-mid-construct-reset** (and only those claim partial
   lexical/UTF-8/CR/DTD state).
3. **Forensic provenance chain**: `pushdiff-run.sh` now refuses to run
   from a dirty worktree (so `candidate_sha`/`court_sha` name a committed
   state) and records cryptographic fingerprints: probe/generator/runner
   sha256, the sha256 of the actual candidate `libxml2.so` binary mounted
   as `/candidate`, and the oracle image ID + repo digest (a local tag is
   mutable; the ID/digest is not). Workflow is now: commit court → run
   from clean tree → commit evidence.

Note: as of this slice the repo has no GitHub status checks / workflow
runs, so nothing in slice 0 is described as CI-verified — the court runs
are the verification.

### Frozen slice-0 baseline (unchanged parser, final)

| metric | value |
|---|---:|
| cells | 4042 |
| diverging cells | 4042 (100%) |
| court/source sha of that run | `d762c286` (see `raw/pushdiff/run.txt`) |

**Provenance note:** the frozen 4042-cell baseline was produced by the
slice-0 court at `d762c286`, whose generator did not yet include the
slice-1 CR-chain documents. `raw/pushdiff/run.txt` therefore records
`candidate_sha=court_sha=d762c286`. Slice 1 EXTENDS the corpus
(`cr-cr`, `cr-cr-cr`, `cr-cr-final`, `cr-cr-tags`, `cr-crlf-mix`), so the
current HEAD's generator yields MORE cells; running it does not reproduce
exactly 4042, by design — slice-0 history is not rewritten. Slice-1 runs
should be read as “slice-0 corpus + slice-1 additions”, with the CR-chain
cells reported separately.

Run: `sh courts/suites/phase16/pushdiff-run.sh` from a clean tree; the
gate is diffs = 0 after the stateful rewrite.

## Slice 1 (in progress) — replay-era class fixes + newly surfaced classes

Step 1/1a: `end_in_lf` trailing-CR deferral (`PushState::pending_crs`,
`parse_chunk`). The withheld CR bytes are COUNTED, not flagged: consecutive
raw CRs must not collapse (upstream keeps every one in the buffer, so
`<a>x\r\ry</a>` fed as `<a>x\r`, `\r`, `y</a>` leaves two unread CRs).
Slice-1 corpus additions `cr-cr.xml`, `cr-cr-cr.xml`, `cr-cr-final.xml`,
`cr-cr-tags.xml`, `cr-crlf-mix.xml` exercise CR→CR→data,
CR→CR→CR→data, CR→CR→final, CR→CR→tags and CR→zero→CR→data under the
existing `b1`/`zK`/`rN` plans (these are slice-1 cells on top of the
frozen slice-0 baseline of 4042, which is not reopened).

The targeted CR behavior is correct: a withheld CR is consumed only when
the call brings bytes or terminates, so a ZERO-LENGTH non-final call
leaves it parked — verified against the oracle across `cr-multi` /
`text-crlf` / `text-cr-chunkend` / `text-lone-cr` × `b1z2` / `b64z2` /
`r17z1`: **zero character events on zero-length calls on both sides**.
This is NOT “class 3 closed”: the cells still diverge on classes 1/5, and
the withheld-CR bookkeeping is transitional scaffolding (the persistent
machine keeps the bytes in the input buffer and parks the cursor before
them instead).

### 5. Character-data callback segmentation (raw CR / CRLF + availability batching)

Extracting each trace's (call → `characters` event) sequence shows a
segmentation model that is more than “split at each CR/LF”. Three distinct
mechanisms are visible in the oracle:

1. **≥300-byte availability gate.** `XML_PARSER_BIG_BUFFER_SIZE` (300): in
   CONTENT, upstream only enters `xmlParseCharDataInternal` when
   `avail >= 300` OR `xmlParseLookupCharData` finds a `<`/`&` delimiter;
   otherwise a non-final call `goto done` without touching the run. Long
   text therefore dispatches in ~300-byte batches (observed: 317/320-byte
   `characters` runs for the 4.4 KiB `text-long` doc fed in 64-byte chunks,
   one every ~5 chunks).
2. **CRLF in the fast path splits.** A raw `0x0D` followed by `0x0A` leaves
   the accelerated scan; `xmlParseCharDataInternal` flushes the run before
   it and consumes the CRLF so the normalized `\n` heads the NEXT callback —
   hence `[one] [\ntwo] [\nthree] [\n]` for `one\r\ntwo\r\nthree\r\n`
   (`text-crlf`, `cr-multi`). Plain LF does NOT split.
3. **Lone CR takes the complex path and MERGES.** A raw CR not followed by
   LF falls out of the fast path into `xmlParseCharDataComplex`, which
   batches differently: lone-CR text yields a merged `[\ntwo\nthree\n]`,
   not one callback per CR (`text-lone-cr`), and consecutive CRs merge
   (`cr-crlf-mix` → `[\n\n\ny]`, `cr-cr` → `[\n\ny]`).

A first implementation attempt (end the `Characters` token at every raw CR)
was measured against the oracle and **reverted**: it matches mechanisms 2
for CRLF documents but diverges on lone/consecutive CRs (where upstream
merges) and cannot reproduce the ≥300-byte batching at all. Mechanism 1 is
inherently coupled to input availability — i.e. to the persistent engine's
buffer/cursor model — so faithful class-5 segmentation lands with that
engine, not as a scanner-local tweak. (It is observable with the whole
document in one chunk, so it is independent of replay but NOT of the
input-availability model.)

## Slice 1 blocker (next work): progressive input decoding

The persistent driver cannot consume input until `InputBuffer` is genuinely
progressive in its ENCODING handling. Two pre-existing defects (they only
stayed hidden because the replay design always re-presented the whole
accumulated buffer to a whole-buffer decoder):

1. **Multi-byte tails are appended raw after conversion.** Once
   `convert_detected_utf16()` has replaced `data` with UTF-8 and set
   `converted_to_utf8`, the next `push_bytes` finds
   `legacy_source_encoding_name() == None` for `Utf16Le`/`Utf16Be`, so it
   falls through and appends the RAW UTF-16 tail onto the converted UTF-8
   buffer.
2. **Encoding detection is not re-run for short prefixes.**
   `push_bytes` re-detects only on `first_real_bytes` or `decl_pending`.
   A 1-byte first chunk (`FF`) makes `detect_bom_and_encoding` default to
   UTF-8 (it cannot match a 2-byte BOM), and the second chunk (`FE 3C 00…`)
   does not re-detect — the BOM is lost. Upstream avoids this by parking in
   `XML_PARSER_START` on a non-final call with too few bytes (`avail < 4`,
   and `avail < 200` for the EBCDIC probe) BEFORE encoding detection.

### Oracle evidence (frozen court VM, `enc-utf16le-bom.xml`, b2 chunks)

```text
ORACLE (correct progressive behavior):
  CALL 0 in=0  (START, parks: encoding unit incomplete)
  CALL 1 in=17 (XML_DECL: BOM seen, transcoded)
  CALL 2 in=6  startDocument; startElementNs local=[a]
  CALL 4 in=7  characters len=1 [x]

CANDIDATE (broken):
  CALL 0 in=1  startDocument; endDocument     <- raw UTF-16 treated as UTF-8
  CALL 1 in=7  startDocument; endDocument     <- never decodes; no element
  ...
```

The same divergence reproduces for UTF-16BE, BOM-less UTF-16, UTF-32BE,
EBCDIC, surrogate pairs, and a document ending with half a code unit.

### Corpus cases added (binary, split by the b1/b2/b3 plans)

`enc-utf16le-bom`, `enc-utf16be-bom`, `enc-utf16le-nobom`,
`enc-utf16be-nobom`, `enc-utf32le-bom`, `enc-utf32be-bom`,
`enc-utf32be-nobom`, `enc-ebcdic` (cp037), `enc-utf16le-surrogate`,
`enc-utf16be-surrogate`, `enc-utf16le-half-end`, `enc-utf16be-half-end`,
`enc-bom-le-only`, `enc-bom-be-only`, `enc-bom-le-half`. Together with the
b1/b2/b3 plans these split the BOM, the first-four-byte signatures, single
code units, and surrogate pairs across calls, and cover the terminating
call carrying half a code unit.

### Fix plan (next slice, before any XML-grammar wiring)

- **Defer detection while undecided.** `detect_bom_and_encoding` must be
  able to report *undecided* (too few bytes + `!terminate`) instead of
  defaulting to UTF-8, and `push_bytes` must re-run detection on every
  subsequent push until a decision is made.
- **Keep the source decoder installed.** For `Utf16*`/`Ucs4*`/`EBCDIC`/
  registry encodings, tails are decoded with a decoder that CARRIES the
  trailing bytes that do not yet form a complete code unit (odd byte,
  half a surrogate pair); the terminating call flushes the carry so a half
  unit at EOF reports the same error as the oracle.
- **Termination must reach the input layer.** `push_bytes` needs the
  `terminate` flag (or an explicit finalize step): the detection deferral
  and the carry flush are both terminate-dependent — an incomplete
  encoding unit with `terminate == 0` is `NeedMoreInput`, not an error.
- **Accounting authority moves to `InputBuffer`.** Whole-buffer conversion
  REPLACES bytes rather than appending, so
  `machine.materialize_input(n)` cannot represent it. `InputBuffer` should
own the materialized/consumed totals (it knows raw arrival, decoder state,
  conversions, rebasing) and `PushMachine` should keep only
  `source_bytes_received` + `total_scan_work`, querying the input for the
  rest when producing the complexity receipt.

## What the divergences are (five systematic classes)

Classes 1–4 share one architectural root cause — the whole-buffer replay
design, which runs a complete-document parser on every non-final call —
and each is independently observable in the push trace. Class 5 is
different in kind: it is a **character-data scanner / SAX event
segmentation** issue, observable with the whole document in a single
chunk, and therefore does **not** require replay or chunk boundaries to
occur. Both matter for a drop-in custodian.

### 1. Lifecycle / event-timing mismatch

The oracle's `xmlParseTryOrFinish` does not fire `startDocument` until it
can leave `XML_PARSER_START` (the `avail < 4` gate on non-final calls) and
never fires `endDocument` on a non-final call — `xmlFinishDocument` runs
only on the terminating call or at a fatal error. The candidate's
whole-buffer re-parse instead runs a complete document lifecycle per call:

- single-byte feeds: oracle makes NO progress (instate stays START, no
  events) while the candidate re-parses `<` / `<a` / `<a/` every call,
  firing `startDocument` (+ `endDocument` via its completion semantics)
  repeatedly, and reports `instate` = CONTENT after truncated calls;
- whole-doc-in-one-non-final-chunk: the candidate fires `endDocument` on
  that call; the oracle defers it to the terminating call.

The 1250-test PHP six-gate does not see this (PHP's expat-compat layer does
not surface startDocument/endDocument timing to userland), which is why it
sealed despite the divergence. SAX-event consumers (lxml `iterparse`,
nokogiri SAX push) DO observe it.

### 2. `NeedMoreInput` treated as EOF / malformed input

Independently observable semantic failure of using a complete-document
parser on an incomplete non-final stream: the candidate emits an error
where upstream simply parks. The slice-0.3.1 pending-UTF-8 / pending-entity
cells prove it — the oracle reaches the reset point with rc=0 err=0 wf=1
while the candidate prematurely reports XML_ERR_INVALID_ENCODING (rc=81)
for `<a>\xC3` / `<a>\xE2\x82` / `<a>\xF0\x9F\x8E` and an entity error for
`<a>&am`. This is not cursor fallout from class 1: it is the missing
`IncrementalResult::NeedMoreInput` semantic (byte exhaustion +
terminate=0 is SUSPENSION, never EOF).

### 3. Trailing-CR `end_in_lf` deferral mismatch

Upstream `xmlParseChunk` strips a trailing `\r` from a non-final chunk and
re-pushes it AFTER `xmlParseTryOrFinish`, so a CRLF pair split across two
chunks still normalizes to one `\n` and a lone trailing `\r` is not
treated as an EOL (or dispatched into that call's text run) until the next
chunk arrives. The candidate appends and parses the `\r` immediately.

### 4. Finished-context REFEED behavior mismatch

After a well-formed document finishes (context at XML_PARSER_EOF), feeding
the document again with terminate=1 makes the ORACLE push the bytes and
raise "Extra content at the end of the document" (XML_ERR_DOCUMENT_END →
errNo 5, wellFormed 0, rc 5). The candidate's `parse_chunk` EOF gate
returns 0 early without pushing or parsing (rc 0, wellFormed 1). gh12254's
parse-twice surface.

## Why this matters for 16.7.8

The O(N²) re-parse design is not only quadratic — it cannot reproduce the
oracle's per-call event timing at chunk boundaries because it re-runs the
whole document lifecycle per call. The stateful incremental engine that
16.7.8 requires (persist the parser state — input cursor, phase, open
element frames — across `xmlParseChunk` calls and only ever scan new
bytes, mirroring `xmlParseTryOrFinish`) is therefore a CORRECTNESS fix as
well as the O(N²) fix. This court is the gate every 16.7.8 slice must keep
green: after each change, `cells=… diffs=0`.

## Acceptance criteria for 16.7.8 (expanded)

G1  oracle == candidate per-call trace (this court, sax2)
G2  oracle == candidate per-call trace (sax1)
G3  all deterministic random chunk plans (rN) green
G4  every-byte boundary coverage (b1 on all docs ≤ 9 KiB) green
G5  no ASan/UBSan/Valgrind findings on the push path
G6  pushscale.c is O(N) (20/40/80 MB curve ~linear)
G7  lxml suite passes (oracle baseline 2007/0) — large iterparse unblocked
G8  nokogiri push/SAX passes
G9  PHP six-gate remains 0 failures (1250/0)
G10 cargo test --lib green (1267)

## Next slices

1. Stateful push driver: park the parser (phase, element frames, tokenizer
   input cursor, namespace scope) across non-final calls; scan only new
   bytes; reproduce the oracle's per-call event/error timing (startDocument
   gating, endDocument-on-terminate, REFEED extra-content, end_in_lf).
2. Re-run this court (must reach diffs=0) + `cargo test --lib` + PHP
   six-gate + lxml/nokogiri gates.
3. Re-run pushscale.c: the 20/40/80 MB curve must go linear.
