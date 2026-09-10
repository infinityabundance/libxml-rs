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

## Slice 0.3 — suspended-reset cells + forced constructor boundaries
(freeze of slice 0; no further court expansion until the engine drives)

Reviewer items closed before the stateful rewrite:

1. **Suspended-reset cells** (`Rs<plan>` / `Rsi<plan>`): document A is fed
   under the plan but NEVER finished — it ends mid-construct with partial
   lexical token / pending UTF-8 / pending CR / open element stack / DTD
   declaration parked — and only then is `xmlCtxtResetPush` called
   (empty, or with whole B). Six truncated-A × unrelated-B pairs
   (`raw-trunc-utf8`, `text-cr-end` (trailing CR pending),
   `starttag-trunc-attr-val-dq`, `doctype-trunc-elem-decl`,
   `endtag-trunc` (open stack), `doctype-trunc-entity`).
2. **Forced constructor boundaries**: new corpus docs
   (`ctor-utf8`, `ctor-utf8-cjk`, `ctor-entity`, `ctor-charref`,
   `ctor-attr`, `ctor-attr-ns`, `ctor-dtd`) plus `Cb5`/`Cb12` plans so a
   constructor-initial chunk ends inside a multibyte sequence, an entity
   reference, an attribute value, or a DTD declaration.
3. **CTOR trace nit**: the constructor record is now `< CTOR ok=1 err=…
   wf=… in=… p=…` (no manufactured rc); `ok=0` when the context is NULL.
4. **Receipt dedup**: the duplicated “Acceptance criteria” heading is
   removed.

### Frozen slice-0 baseline (unchanged parser)

| metric | value |
|---|---:|
| cells | 3962 |
| diverging cells | 3962 |

All remaining divergence traces back to the three systemic classes
(startDocument/endDocument call timing, REFEED-after-finish,
end_in_lf CR deferral) plus their cursor/state consequences (nameNr
inflation, p/l/col drift, rc-vs-errNo on refused calls). Every cell is
regenerable with `sh courts/suites/phase16/pushdiff-run.sh`.

## What the divergences are (three systematic classes)

### 1. The candidate fires startDocument / endDocument on the wrong calls

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

### 2. REFEED on a finished context

After a well-formed document finishes (context at XML_PARSER_EOF), feeding
the document again with terminate=1 makes the ORACLE push the bytes and
raise "Extra content at the end of the document" (XML_ERR_DOCUMENT_END →
errNo 5, wellFormed 0, rc 5). The candidate's `parse_chunk` EOF gate
returns 0 early without pushing or parsing (rc 0, wellFormed 1). gh12254's
parse-twice surface.

### 3. `end_in_lf` — a chunk ending in `\r` (no deferral)

Upstream `xmlParseChunk` strips a trailing `\r` from a non-final chunk and
re-pushes it AFTER `xmlParseTryOrFinish`, so a CRLF pair split across two
chunks still normalizes to one `\n` and a lone trailing `\r` is not
treated as an EOL (or dispatched into that call's text run) until the next
chunk arrives. The candidate appends and parses the `\r` immediately.

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
