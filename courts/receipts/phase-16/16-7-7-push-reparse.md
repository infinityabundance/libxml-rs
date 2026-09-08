# Phase 16.7.7 addendum — push-parse re-parse cost (lxml iterparse blocker)

Date: 2026-09-08 (found while re-running the lxml candidate gate after the
§16.7.7 fixes)
Commit: (this commit)

## Finding

The lxml candidate gate stalls (effectively hangs) on
`test_etree.py::test_very_large_sourceline_iterparse` (375 push chunks of
6 MB of newline text + tags; 2.2 GB total). The oracle (libxml2 2.15.3)
passes the full lxml suite (2007 tests, 0 failures). faulthandler stack of
the stalled run: the test was executing
`test_etree.py:4183 test_very_large_sourceline_iterparse` inside
`etree.iterparse` — the parser made no progress for > 20 minutes.

## Root cause: per-chunk whole-input re-parse (pre-existing design)

`helpers.rs parse_chunk` implements the SP-14.3.1-6 push semantics (PHP
expat-compat event delivery) by RE-PARSING THE WHOLE ACCUMULATED INPUT on
every non-final `xmlParseChunk` — a silent probe parse plus a delivery
parse (with events suppressed below `delivered_bytes`), each on a fresh
duplicate of the base buffer. When every chunk ends inside an unterminated
character-data run (newline text with the run still open at the chunk
boundary), the re-parse re-scans the text from the run start every chunk:
chunk i costs O(i), the whole feed costs O(N²).

Measured with `courts/suites/phase16/pushscale.c` against the release
candidate (this commit) and the oracle, 1 MB newline chunks + `<br/>`
separators (all under the 10 MB text-node limit; oracle shape identical):

| chunks (MB) | oracle (s) | candidate (s) |
|-------------|-----------|---------------|
| 20 (20 MB)  | 0.01      | 6.4           |
| 40 (40 MB)  | 0.03      | 24.6          |
| 80 (80 MB)  | 0.05      | 96.0          |

The oracle is linear (~1.6 GB/s: it keeps parser state across chunks);
the candidate is quadratic (4× time per 2× input). Plain (non-push)
parsing of the same content is fast on the candidate (20 MB newlines via
xmlReadFile: 0.17 s), so the cost is specific to the push re-parse.

## Why this surfaced now

The §16.7.7 error-trampoline crash fix (release-only free(stack-pointer)
on encoding-error raises; `inlateout` operand declarations in
ch_call0/1/2) made the push parser CORRECT where the pre-fix build
silently short-circuited (crashed / aborted early / hit resource limits
without processing). The pre-fix lxml gate crashed at rc=134 (the
double-free) before reaching this test; the fixed build reaches it and
then pays the pre-existing quadratic re-parse. This is NOT a regression of
the fix — it is a newly-visible pre-existing architectural cost that is
also the Phase-16.12.3 lxml `iterparse` consumer-benchmark blocker.

## Status / next steps

- NOT fixed in this commit. The fix is a stateful push resume (keep the
  parser's element stack / input position across non-final chunks like
  upstream) or a delivered-prefix fast-forward that replays only
  stack-relevant state instead of re-scanning text — a substantial change
  to `helpers.rs parse_chunk` with deep SP-14.3.1-6 regression risk
  (PHP expat-compat event semantics, xml_parse isFinal=false flows,
  eager-partial delivery, hostile-failure courts).
- Interim: the lxml candidate gate cannot complete while this stands; PHP
  (small documents) and DOM paths are unaffected. Tracked as a Phase-16
  push-path performance item (also §16.12.3 iterparse).

## Gates on the committed fix (unchanged by this addendum)

- cargo test --lib: 1267 pass / 0 fail.
- §16.7.7 court: G1 PASS, G2 trees byte-identical (previously sealed).
- Encoding-error / `]]>` / NUL / name / end-tag parity inputs: byte-identical
  with the oracle (re-verified after the restore).
