# Phase 16.7.8 slice 0 — push-parser differential court + baseline divergence inventory

Date: 2026-09-09
Commit: (this commit)
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

## Baseline result (candidate == current HEAD before 16.7.8)

| metric | value |
|---|---|
| cells (document × plan × sax-mode) | 2624 |
| diverging cells | 2619 |
| event-level divergences (startDocument/startElement/characters/error records differ between calls) | 2461 |
| REFEED-only divergences (identical feed/events; only the post-finish refeed differs) | 74 |
| other (instate/rc line diffs only) | 84 |

Raw streams: `raw/pushdiff/oracle-*`, `cand-*`, per-cell diffs in
`raw/pushdiff/diffs/`, `summary.txt`.

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

## Next slices

1. Stateful push driver: park the parser (phase, element frames, tokenizer
   input cursor, namespace scope) across non-final calls; scan only new
   bytes; reproduce the oracle's per-call event/error timing (startDocument
   gating, endDocument-on-terminate, REFEED extra-content, end_in_lf).
2. Re-run this court (must reach diffs=0) + `cargo test --lib` + PHP
   six-gate + lxml/nokogiri gates.
3. Re-run pushscale.c: the 20/40/80 MB curve must go linear.
