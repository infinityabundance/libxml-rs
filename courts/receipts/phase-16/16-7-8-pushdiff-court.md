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

Fixed-width/byte-wise (19 documents): `enc-utf16le-bom`, `enc-utf16be-bom`,
`enc-utf16le-nobom`, `enc-utf16be-nobom`, `enc-utf32le-bom`,
`enc-utf32be-bom`, `enc-utf32be-nobom`, `enc-ebcdic` (cp037),
`enc-utf16le-surrogate`, `enc-utf16be-surrogate`, `enc-utf16le-half-end`,
`enc-utf16be-half-end`, `enc-bom-le-only`, `enc-bom-be-only`,
`enc-bom-le-half` (+ the court-hardening additions). Together with the
b1/b2/b3 plans these split the BOM, the first-four-byte signatures, single
code units, and surrogate pairs across calls, and cover the terminating
call carrying half a code unit.

Registry-served multibyte/stateful (12 documents, added with the persistent
decoder slice — see below): `enc-shift_jis`/`-long`, `enc-euc-jp`/`-long`,
`enc-iso2022-jp`/`-long`/`-shifted`, `enc-ucs2`/`-half`/`-half-unit-only`,
`enc-shift_jis-half`/`-half-unit-only`.

### Fix plan (next slice, before any XML-grammar wiring)

#### Acceptance: decoder-specific vs full-trace

These cells run the COMPLETE push-parser trace, so the decoder fix
cannot take them to `diffs = 0` on its own: the replay architecture
independently differs from upstream on startDocument/endDocument timing,
`NeedMoreInput` behavior, `ctxt->instate`, `cur-base`/`nameNr` progression,
REFEED and class-5 segmentation. A fully correct decoder can therefore
leave the full count unchanged.

```text
Decoder-specific gate (expected GREEN after the decoder work):
  VALID cases    no premature encoding error; decoded element names,
                 attributes and character payloads match the oracle;
                 encoding boundaries do not change the semantic payload
  INVALID cases  incomplete unit + !terminate parks (no error);
                 incomplete unit + terminate yields the oracle's error
                 domain/code (XML_FROM_I18N 81 "Invalid bytes in character
                 encoding" for the isolated half-unit case); no malformed-XML
                 error supersedes the isolated decoder error
  DETECTION      BOM/signature detection at the oracle-compatible boundary;
                 EBCDIC stays START through avail 199 and progresses at 200

Full pushdiff trace (expected to STAY RED until the persistent push-parser
lifecycle/state wiring lands): the full-corpus count above.
```

That is the causal isolation: the decoder work is judged on decoder
observables; the full-trace byte identity is the later persistent-engine
gate.

#### Decoder slice IMPLEMENTED — and the gate is now executable

Landed in three commits:

```text
9fac5d17  progressive source decoder in InputBuffer (park/decide/carry)
8c6f9c8b  decoder-gate court + two detection corrections
a42145fb  PI availability gate + document-level char-data diagnostics
```

`InputBuffer` now owns a `SourceDecoding` state (`WholeBuffer` / `Parked` /
`Decided`) and a carry (`pending_source`). A push session starts `Parked` and
re-runs detection on every call until it can decide, reproducing
`xmlParseTryOrFinish`'s `XML_PARSER_START` gates exactly — `(!terminate &&
avail < 4) goto done` and the EBCDIC signature `4C 6F A7 94` waiting for 200
bytes. A parked non-final call parses nothing, fires nothing and raises
nothing; a terminating call always decides. Once decided, complete encoding
units are decoded incrementally (UTF-16 carries an odd byte or a lone high
surrogate, UTF-32 carries 0–3 bytes) — never a whole-buffer re-decode, which
would only move the replay parser's O(N²) down one layer. Termination reaches
the input layer: an incomplete unit on a non-final call is suspension; the same
unit on a terminating call is the encoder flush (`xmlParserCheckEOF` →
`xmlCtxtErrIO`) → `XML_FROM_IO` 81 "Invalid bytes in character encoding".
`InputBuffer` also owns the source/materialized accounting beside the position
it already owned; consumption accounting stays machine-side until the
persistent driver exists, because the transitional replay parses a DUPLICATE of
the base buffer and so has no meaningful single consumed cursor yet.

Unit tests (9) cover the park gate, split UTF-8/UTF-16 BOMs, the EBCDIC
200-byte threshold, surrogate-pair and half-unit splits, definite-vs-suspension
error semantics, and **partition equivalence** — any source chunk partitioning
materializes exactly the whole-source UTF-8 stream.

#### The gate execution (`PUSHDIFF_FILTER=enc-`, candidate `a42145fb`)

```text
cells=355  diffs=355          full trace: still RED (expected)
pushdiff decoder gate: 355/355 cells green
  (decoded content, encoder error, FINAL well-formedness)
  (informational: 345/355 cells also differ in the REFEED line — class 4)
  (informational: 27/355 cells agree on well-formedness but report a
   different parser diagnostic code)
```

`courts/suites/phase16/pushdiff-decoder-gate.py` projects each recorded trace
onto what the source decoder owns: the decoded SAX content (adjacent
`characters` runs merged — segmentation is class 5), the encoder error
(`dom=8 code=81`) if any, and the `FINAL` outcome. Its provenance is recorded
in `run.txt` (`decoder_gate_sha256`, `run_decoder_gate_sha256`) and the raw
traces it derives from are the run's recorded `oracle-*`/`cand-*` files.

Representative evidence in `raw/pushdiff/sample/`:

| sample | what it shows |
|---|---|
| `enc-ebcdic-long__b1__oracle-calls-190-205.txt` | oracle parks (`in=0`, `p=0`) through CALL 198; CALL 199 (avail 200) fires `startDocument`+`startElementNs [a]`, `in=7` — the 200-byte EBCDIC gate |
| `corpus_enc-ebcdic-long.xml__b1.diff` | the candidate now matches the oracle byte-for-byte through CALL 199 (the diff starts at line 403); the remaining difference is class-5 character-run segmentation |
| `corpus_enc-utf16le-half-unit-only.xml__b2.diff` | the isolated truncated unit: identical events, identical `error dom=8 code=81 level=3 … i2=9`, identical `FINAL 0 rc=81 err=81 wf=0 in=-1 p=8 l=1 col=9`; only `nameNr` differs |
| `corpus_enc-utf16le-bom.xml__b1.diff` | a BOM-split UTF-16LE document decodes to the oracle's events; only lifecycle timing/`instate`/`nameNr` remain (classes 1–2) |
| `corpus_enc-utf16be-nobom.xml__b1.diff` | BOM-less UTF-16: the `<?` prefix now parks until `?>` is available instead of raising PI_NOT_STARTED / RESERVED_XML_NAME |
| `corpus_enc-utf32be-bom.xml__b1.diff` | `00 00 FE FF` is NOT a UTF-32 BOM upstream: both sides reject the document, with different parser diagnostics (see below) |
| `decoder-gate.txt` | the gate output above |

#### Three corrections the decoder work produced

1. **No UTF-32/UCS-4 BOM exists.** Neither `xmlDetectEncoding`
   (parserInternals.c) nor `xmlDetectCharEncoding` (encoding.c, appendix F)
   has a UTF-32 BOM case; `FF FE 00 00` is a UTF-16LE BOM whose first decoded
   character is U+0000 and `00 00 FE FF` falls through to default UTF-8. The
   oracle confirms it (`enc-utf32??-bom`: "Start tag expected, '<' not found",
   rc 4). BOM-LESS UCS-4 (`3C 00 00 00` / `00 00 00 3C`) is still detected by
   both. The candidate used to treat both BOMs as UTF-32 and parse the
   document cleanly.
2. **A `<?` construct is only scanned once its `?>` is available.** Every
   PI-bearing state gates `xmlParsePI` behind `(!terminate) &&
   (!xmlParseLookupString(ctxt, 2, "?>", 2))`, so a non-final call defers the
   whole construct. The candidate scanned optimistically and raised
   PI_NOT_STARTED (46) / RESERVED_XML_NAME (64) on truncated prefixes.
   Correct decoding exposed this: UTF-32 materializes one character per call,
   so `<?`, `<?x`, `<?xml` are reached immediately.
3. **Document-level character data does not scan char data.** Upstream's MISC
   state sends a non-'<' byte straight to `XML_PARSER_START_TAG`
   ("Start tag expected", code 4) and never runs `xmlParseCharData` there; the
   candidate's tokenizer could record `XML_ERR_INVALID_CHAR` ("PCDATA invalid
   Char value 0") for an invalid byte in the run and supersede it.

#### Registry-served codecs are now genuinely progressive (`581a74c5`)

The fixed-width decoder paths carried incomplete units correctly, but the
registry tail route (`Encoding::Other(_)`) decoded **each chunk
independently** — `decode_whole_buffer_declared` built a fresh `encoding_rs`
decoder per call and passed `last = true`. That is wrong for every codec that
is not a byte-wise mapping:

```text
Shift_JIS  82 A0 is one character
  CALL 1: ... 82   -> an independent decode calls 82 malformed
  CALL 2: A0 ...   -> the raw lead byte was materialized; the character is lost

ISO-2022-JP  ESC $ B (JIS state) ... ESC ( B
  a boundary can land with NO incomplete byte sequence at all, so no byte
  carry can help — only the decoder's escape state survives
```

`Encoding::Other` is no longer one homogeneous category. `RegistryKind`
classifies the resolved (alias-canonicalized) name:

| kind | encodings | decoding |
|---|---|---|
| `Streaming` | Shift_JIS/SJIS/CP932, EUC-JP, ISO-2022-JP | a **persistent** `encoding_rs::Decoder`, installed at decision time and reused for every later chunk |
| `Ucs2Le` | `UCS-2` | 2-byte unit carry |
| `Ucs4(le/be)` | `ucs-4`, `ucs-4be` | 4-byte unit carry |
| `ByteWise` | ISO-8859-x, windows-1252, EBCDIC, unresolved names | tail-wise through the registry handler (unchanged) |

Decoding always feeds `last = false`, so an incomplete unit is buffered inside
the decoder; `last` is applied as a separate empty-input flush, mirroring
upstream's `xmlCharEncInput(flush = 0)` plus `xmlParserCheckEOF`'s `flush = 1`.
A definite invalid unit latches the immediate encoder error; a still-held unit
on a terminating feed latches the truncation. `convert_declared_native_encoding`
now takes the call's `terminate` so a declared codec whose first (and only)
feed is final reports the flush too.

**A trap found on the way**: `encoding_rs`'s two-byte decoders CLEAR their
buffered lead byte when called with an EMPTY slice and `last = false`
(`ShiftJisDecoder`'s prolog sets `self.lead = None` before returning
`InputEmpty`). The decoder here is therefore never called that way: only real
bytes are fed `last = false`, and the single empty-slice call is the
`last = true` flush.

**A first version was falsified by the court.** It split the buffer at the
declaration and decoded only the body, on the theory that upstream switches
the encoding with `cur` past the declaration. The new `enc-ucs2*` documents
show the oracle does the opposite for a unit-aligned codec — it REJECTS the
document (`XML_ERR_SPACE_REQUIRED` 65 "Blank needed here" +
`XML_ERR_'?>' expected` 57, wellFormed 0, `p=3`, `col=38` = the ASCII
declaration): the switch converts the input buffer and the parser re-reads it,
so the already-consumed declaration is decoded as UCS-2 as well and the re-scan
fails. Declared codecs now decode the WHOLE buffer from byte zero, the
candidate rejects these documents too (well-formedness parity), and the exact
diagnostic remains parser-side.

#### Corpus families added for this (12 documents, +210 cells)

```text
enc-shift_jis / -long           2-byte characters split at every boundary
enc-euc-jp / -long              same, EUC-JP
enc-iso2022-jp / -long          ESC $ B ... ESC ( B, split inside the ESC runs
enc-iso2022-jp-shifted          shifted in with no ASCII text after the shift, so a
                                boundary can land between the JIS pairs and ESC ( B
                                with NO incomplete bytes: state only
enc-ucs2 / -half                declared UCS-2 (the shape the oracle rejects)
enc-shift_jis-half / -half-unit-only   dangling half unit at EOF (combined /
                                isolated flush)
enc-ucs2-half / -half-unit-only same for UCS-2
```

Corpus totals: 31 encoding documents, 565 cells under `PUSHDIFF_FILTER=enc-`
(was 19 / 355).

#### Gate execution (candidate `581a74c5`)

```text
cells=565  diffs=565             full trace: still RED (expected)
pushdiff decoder gate: 565/565 cells green
  informational: 555/565 also differ in the REFEED line (class 4)
  informational:  84/565 agree on well-formedness, different parser diagnostic
```

Every encoding document is fully green, including all four new families:

```text
19/19 enc-shift_jis               19/19 enc-iso2022-jp-shifted
19/19 enc-shift_jis-half          19/19 enc-iso2022-jp-long
19/19 enc-shift_jis-half-unit-only 19/19 enc-ucs2
13/13 enc-shift_jis-long          19/19 enc-ucs2-half
19/19 enc-euc-jp                  19/19 enc-ucs2-half-unit-only
13/13 enc-euc-jp-long             19/19 enc-utf16le-bom  (+ all other fixed-width)
19/19 enc-iso2022-jp              13/13 enc-ebcdic-long
```

The 84 informational cells are the parser-diagnostic class only: 27
`enc-utf32??-bom` (raw NUL/invalid bytes at the document start, below) and 57
`enc-ucs2*` (the diagnostic the oracle reports while rejecting a declared
unit-aligned stream). Neither gates the decoder contract.

#### Open, newly isolated: parser diagnostic on raw NUL/invalid bytes

The 27 informational cells are exactly `enc-utf32??-bom`, where the oracle
rejects the document (`XML_ERR_DOCUMENT_EMPTY` "Start tag expected", rc 4) and
the candidate also rejects it but with `XML_ERR_INVALID_CHAR` (9) or
`XML_ERR_NAME_REQUIRED` (68). Both agree the document is malformed; only the
diagnostic differs. The root cause is that the candidate's tokenizer is
phase-agnostic and scans character data before the phase-driven parser can
reach `XML_PARSER_START_TAG` — a replay-architecture property (the persistent
driver knows its phase and will not scan char data at document level), not a
decoding one. It is decoder-independent: any input whose first bytes are NUL or
invalid UTF-8 reproduces it.

#### Implementation plan (retained as the pre-implementation record)

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
G10 cargo test --lib green (1291 as of the decoder slice)

Decoder sub-gate (executable, `pushdiff-decoder-gate.py`, expected GREEN now):
decoded content + encoder error + FINAL well-formedness per `enc-*` cell —
currently 565/565 on the grown decoder corpus, with the full trace still red
as designed.

## Slice 1 (steps 5 + 5b) — the persistent driver exists (court-only)

**What landed.** `src/xml/parser/pushdrive.rs`: the first real persistent push
DRIVER. It is not a mock and not a wrapper around replay — it owns the control
loop and reuses the existing grammar primitives verbatim
(`XmlParser::parse_element_start`, `close_open_element`, `sax_*`, the
tokenizer). `state.rs` does not become the pull parser and `pushdrive.rs` does
not become a second XML grammar.

It is a structural mirror of upstream's TWO pieces, which matters because the
split is what produces the lifecycle behaviour:

```text
xmlParseTryOrFinish(ctxt, terminate)   -> drive_persistent's loop
  while (disableSAX == 0):
    avail = end - cur; if (avail < 1) goto done;
    switch (instate) { START | XML_DECL | MISC/PROLOG/EPILOG |
                       START_TAG | CONTENT | END_TAG | EOF }

xmlParseChunk's terminate block        -> terminate_document()
  instate != EOF && != EPILOG -> TAG_NOT_FINISHED / DOCUMENT_EMPTY
  else                        -> xmlParserCheckEOF(DOCUMENT_END)
  if (instate != EOF) { instate = EOF; xmlFinishDocument(); }
```

`endDocument` is fired by the TERMINATE BLOCK (`xmlFinishDocument`), not by
`xmlParseTryOrFinish`. That is the mechanical reason a complete document in a
non-final chunk does not finish (class 1), and why a REFEED onto a finished
context raises `XML_ERR_DOCUMENT_END` on the terminating call with the
offending bytes still unread (class 4). The earlier version of this driver had
this wrong (it fired `endDocument` from the epilog state) and was rebuilt
around the actual structure. Two consequences also fell out: the
"`Start tag expected`" diagnostic is `XML_ERR_DOCUMENT_EMPTY` (4), not an
internal error; and `START -> XML_DECL -> MISC` is a real three-state
progression, not collapsed.

**The invariant it establishes.**

> A byte the driver has consumed is never scanned again, and an unconsumed
> byte is inspected a BOUNDED number of times.

The FIRST half is availability gating: the driver looks ahead for a
construct's terminator and only then consumes the construct. A construct that
is not yet complete parks with the cursor exactly where it was — **no rewind,
no re-scan of committed bytes, no `suppress_until`**.

The SECOND half is the correction that made this step necessary. The first
implementation re-ran each availability scan from the top of the pending
construct on every call. Upstream does NOT do that: it persists
`ctxt->checkIndex` (and `ctxt->endCheckState`, the open-quote state, for the
quote-aware `xmlParseLookupGt`), and `xmlParseLookupString` deliberately
rescans only `strLen - 1` bytes of overlap. Without that state, "consumed at
most once" does NOT imply global O(N) — it is quadratic in the length of a
single construct (one huge attribute value, or one huge delimiter-free text
run). The candidate now carries the equivalent state in `ParkedConstruct`, and
the receipt below proves the difference numerically.

A related ownership point: the `avail < 4` gate in `XML_PARSER_START` is tested
upstream BEFORE `xmlDetectEncoding`, on RAW source bytes. Re-testing it over
MATERIALIZED bytes is wrong for UTF-16/UCS-4, where four source bytes
materialize as one or two (`FF FE 3C 00` -> `<`). The input layer already owns
that gate (`SourceDecoding::Parked`), so the driver defers to it rather than
keeping a second, materialized-byte copy of the same rule.

**Accounting ownership.** The INPUT owns all three byte domains.
`InputBuffer` alone sees raw arrivals, knows about source decoding,
whole-buffer re-materialization (transcoding may REPLACE rather than append)
and rebasing; the machine ADOPTS its `source_bytes_received()` /
`materialized_bytes()` / `pos()` through `PushMachine::sync_input_totals`
instead of accumulating deltas a replacement would corrupt — so the driver no
longer keeps a second source counter of its own. A backwards mirror latches
`accounting_violation`. (`InputBuffer::pos()` is `(line, col, byte_offset)` —
the byte offset is the THIRD element; reading the first gives the line number,
which is the bug this court caught on its first run.)

**One namespace authority.** `PushMachine` no longer carries an `ns_scope`
vector. The real stack is `XmlParser::ns_scope`, which survives across calls
because the same `XmlParser` does; `ParkedElement::ns_scope_mark` indexes into
it. Two candidate namespace stacks would have become a genuine ambiguity as
soon as attributes/namespaces were exercised.

**Construct continuation state.** `ParkedConstruct` separates an unfinished
LEXICAL construct from an open ELEMENT (`ParkedElement`):
`BetweenTokens | Gt { checked, quote } | CharData { checked } |
String { checked, needle, needle_len } | Char { needle, checked }`. `checked`
is an absolute offset into the base input, valid only while the cursor has not
moved past it; `quote` is upstream's `endCheckState`. The continuation is
cleared whenever the cursor advances (upstream clears `checkIndex` when the
construct is consumed).

### Scope of this slice (deliberately bounded)

```text
START      -> the raw-source availability gate (owned by the input layer)
XML_DECL   -> startDocument (declaration parsing is the next slice)
MISC       -> whitespace, then the root start tag
START_TAG  -> simple / self-closing / nested start tags
CONTENT    -> character data, child start tags, end tags
END_TAG    -> end tags
EPILOG     -> whitespace, then the terminating transition to EOF
EOF        -> REFEED ("Extra content at the end of the document")
```

Attributes and namespaces are NOT special-cased and are NOT limited here: they
come from reusing `parse_element_start` verbatim, so they work — the court
exercises `<a p="v"/>` and `<a xmlns:x="urn:u"><x:b/></a>` under every
partition to keep that honest rather than assumed.

XML declarations, comments, PIs, CDATA and DOCTYPE return
`StepOutcome::Unsupported` — a LOUD internal error, never a silent fallback to
replay. Their availability scans ARE implemented (`lookup_string`,
`lookup_char`), so those constructs park correctly instead of mis-scanning,
and the machinery is ready for them. Same for entity/character references
(`&`). The `end_in_lf` CR deferral of class 3 is not modeled.

Two remainders are documented rather than silently omitted:

1. `xmlParserCheckEOF`'s encoder-flush half (a truncated multibyte sequence
   left pending by a terminating call). The decoder already latches it
   (`InputBuffer::source_truncated`); the driver must raise it before any
   context flips over.
2. Exact class-5 character-data SEGMENTATION. The `>= 300`-byte rule IS
   implemented structurally (`avail < XML_PARSER_BIG_BUFFER_SIZE` gates the
   lookup; at or above it the run is parsed without waiting for a delimiter,
   with the continuation cleared either way), which is why long text runs cost
   one inspection per byte — but CR patching and the exact callback split
   positions at that boundary are not yet claimed as parity.

`helpers::parse_chunk` is **untouched**: it still replays, and the full push
trace court below is therefore still red. That is the design, not a regression.

### The court (`pushdrive::tests`)

Five documents x seven plans. Plans: `b1`, `b2`, `b3`, `b5`, deterministic
`random`, `whole-nonfinal` (the entire document in one non-final chunk),
`whole-inline-final`, plus constructor-chunk partitioning and zero-length
non-final / terminating calls.

| Property | How it is checked |
|---|---|
| Partition equivalence | tree == the recursive whole-document parse, for every plan |
| Lifecycle | final phase `XML_PARSER_EOF`; `startDocument`/`endDocument` fired |
| No duplicate/replayed events | dispatched-event count is exactly `1 + start/end per element + characters + 1`, for every plan |
| Forward-only | consumption never moves backwards |
| **Prefix is dead** | the consumed prefix is overwritten with NUL after EVERY call; tree, per-call trace and event count must be byte-identical to the unpoisoned run |
| **Construct not rescanned** | long-single-construct courts: a 512 KiB attribute tag, a 4096-byte text run, and the 1/2/4/8 MiB curve — bounded `scan_work`, flat ratio |
| Liveness | every `drive_step` must change (consumed, phase, events, stack depth) or park |
| Attributes / namespaces | `<a p="v"/>` and `<a xmlns:x="urn:u"><x:b/></a>` under every plan |
| Class 1 | a complete document in ONE non-final chunk does NOT fire `endDocument` |
| Class 1b | non-final feeds of `<` / `<a` / `<a/` leave `instate == START` with zero events |
| Class 4 (REFEED) | refeed onto a finished document raises `XML_ERR_DOCUMENT_END` on the TERMINATING call, and the offending bytes stay UNREAD (`consumed` unchanged) |
| Diagnosis | a non-`<` document start is `XML_ERR_DOCUMENT_EMPTY` (4); an unfinished element is `XML_ERR_TAG_NOT_FINISHED` (77) |

### Complexity receipt (the architectural proof)

`cargo test --lib -- --ignored --nocapture scan_work_curve_is_linear`, three
workload shapes at 1/2/4/8 MiB, 64 KiB chunks:

| workload | MiB | materialized | scan_work | scan_work/materialized | events |
|---|---:|---:|---:|---:|---:|
| many small tags (`<abcdefgh/>` xN) | 1 | 1048582 | 2001837 | 1.9091 | 190654 |
| many small tags | 2 | 2097157 | 4003662 | 1.9091 | 381304 |
| many small tags | 4 | 4194307 | 8007312 | 1.9091 | 762604 |
| many small tags | 8 | 8388607 | 16014612 | 1.9091 | 1525204 |
| **one huge start tag** (8 MiB quoted attribute) | 1 | 1048585 | 2097169 | 2.0000 | 4 |
| one huge start tag | 2 | 2097161 | 4194321 | 2.0000 | 4 |
| one huge start tag | 4 | 4194313 | 8388625 | 2.0000 | 4 |
| one huge start tag | 8 | 8388617 | 16777233 | 2.0000 | 4 |
| **one huge text run** | 1 | 1048583 | 1048592 | 1.0000 | 21 |
| one huge text run | 2 | 2097159 | 2097168 | 1.0000 | 37 |
| one huge text run | 4 | 4194311 | 4194320 | 1.0000 | 69 |
| one huge text run | 8 | 8388615 | 8388624 | 1.0000 | 133 |

Every ratio is constant across an 8x size range and `scan_work` exactly doubles
as the document doubles — including for the single 8 MiB start tag, which is
the case the missing continuation state would have made quadratic (a
restart-from-`<` scan averages half the tag length per call, i.e. tens of GiB
of work for that one document). The huge-text-run events row grows linearly
(21/37/69/133), which is the `>= 300`-byte batching behaving as upstream
describes.

The `many small tags` ratio is 1.9091 here versus 2.0001 before step 5b: the
continuation-aware `lookup_gt` skips the leading `<` exactly as upstream does
(`checkIndex == 0 -> cur + 1`), so it no longer re-examines it. A bounded-ratio
version over 128/512 KiB runs in the default suite for all three shapes, so the
property cannot silently rot.

**`cargo test --lib`: 1312 passed / 0 failed / 2 ignored.**

### Drift-proof headline counts

`tools/evidence/readme_counts.py` now owns the hand-written "Latest gates"
`cargo test --lib` headline (`GATES_RE`, anchored to that exact fragment). That
line had drifted by hand four times (1267 -> 1279 -> 1282 -> 1301 -> 1308)
because it sat outside the generated markers and nothing checked it; `--check`
now fails on it, verified by deliberately corrupting the line and observing
exit 1.

### No-regression evidence (full court, before vs after)

The driver is reached only from `pushdrive::tests`; `helpers::parse_chunk` and
the progressive source decoder are untouched. That claim was MEASURED, not
asserted: the full unfiltered court was run at both commits either side of the
change from a pristine tree.

| | pre `93fd8282` | post `bad79052` |
|---|---:|---:|
| docs / cells | 257 / 4702 | 257 / 4702 |
| diverging cells | 4702 | 4702 |

The court output is byte-identical (all 4702 `DIFF` lines, same order, same
summary); only `candidate_sha`, `court_sha` and the candidate binary sha256
differ. Provenance: `courts/receipts/phase-16/raw/pushdrive-step5/`
(`pre-run.txt`, `post-run.txt`, `README.md`). Note this is also the first
full-corpus baseline at 257 documents — the frozen slice-0 baseline (4042
cells) was a smaller corpus, and the decoder slice only ran the court filtered
to 31 `enc-*` documents (565 cells).

### Explicitly NOT claimed

- Oracle parity for the push trace. The driver has not been compared to
  libxml2 per-call output yet; `pushdiff-run.sh` still reports the replay
  baseline because `parse_chunk` is still replay. This is a
  court-only foundation commit, and the next slice is a shadow court that
  captures `instate`, cursor, line/col, `nameNr`, well-formed/error state and
  SAX events from the oracle for these same small documents.
- Consumer surfaces (PHP six-gate, lxml, nokogiri) are unaffected by
  construction: `parse_chunk` and the decoder were not modified.
- Class 3 (`end_in_lf`), exact class-5 segmentation, and the encoder-flush half
  of `xmlParserCheckEOF` — see the remainders above.

## Next slices

0. ~~Progressive source decoder + executable decoder gate.~~ DONE
   (`9fac5d17`, `8c6f9c8b`, `a42145fb`, `2ffa74ea`, `581a74c5`; gate 565/565
   on the grown decoder corpus, full trace still 565/565 as predicted).

1. Stateful push driver: park the parser (phase, element frames, tokenizer
   input cursor, namespace scope) across non-final calls; scan only new
   bytes; reproduce the oracle's per-call event/error timing (startDocument
   gating, endDocument-on-terminate, REFEED extra-content, end_in_lf).

   1a. ~~The driver exists and is proven court-only.~~ DONE
       (`pushdrive.rs`; forward-only + liveness + prefix-poisoning +
       partition-equivalence courts).
   1b. ~~Availability scanning is genuinely incremental.~~ DONE (step 5b:
       `ParkedConstruct` carries upstream's `checkIndex`/`endCheckState`, the
       `START`/`XML_DECL` ownership boundary is fixed, and three workload
       shapes are proven flat at 1/2/4/8 MiB — including one huge start tag,
       which is the quadratic case the missing state would have hidden).
   1c. An oracle-facing SHADOW COURT for the existing small documents: capture
       `instate`, cursor, line/col, `nameNr`, well-formed/error state and SAX
       events from libxml2 2.15.3 for the same cases (rather than only
       comparing the driver to the candidate's own recursive parser). This is
       what will expose state-machine mistakes long before the production
       flip.
   1d. Declarations and PIs, with real `XML_PARSER_XML_DECL` handling; then
       comments and CDATA reuse the `lookup_string` machinery from 5b.
   1e. DOCTYPE / internal subset (upstream's internal-subset scanner also
       persists `checkIndex` plus a richer `endCheckState`), then entity
       references.
   1f. Only then flip WHOLE contexts from replay to persistent at a point
       selected BEFORE the first observable event — never mid-document — and
       immediately re-run this court + PHP + lxml + Nokogiri + pushscale.
       Replay state starts being DELETED at that point, not merely bypassed.
2. Re-run this court (must reach diffs=0) + `cargo test --lib` + PHP
   six-gate + lxml/nokogiri gates.
3. Re-run pushscale.c: the 20/40/80 MB curve must go linear.
