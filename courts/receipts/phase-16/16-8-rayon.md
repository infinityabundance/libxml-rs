# Phase 16.8 — Rayon multicore parallel blocking: seal receipt

Commit: (this commit)
Date: 2026-09-13
Phase: 16.8 (§16.8.1–§16.8.8). Candidate SHA at measurement time:
`2670f456` + this change set (working tree; the differential and oracle
matrices record `worktree_dirty=yes` for the exploratory run — the clean-seal
re-run is recorded in `raw/16-8-differential/run.txt` after the commit).

This receipt follows the §16.4 discipline (before / patch / after / semantic
gates / delta, `PHASE-16-4-PROFILING.md`) and the
`atlas/PERFORMANCE_ATLAS.md` row schema (`provider`, `backend`, `thread_count`,
`rss_kib`, `result_hash`, `speedup = oracle_time / candidate_time`).

Raw evidence:

| Artifact | Path |
|---|---|
| Isolated scan scaling | `cargo test --release --lib -- ...timing_probe -- --ignored --nocapture` |
| Size × thread matrix | `courts/receipts/phase-16/raw/16-8-parbench/matrix.csv` |
| Dense (tokenizer-bound) matrix | `.../raw/16-8-parbench/matrix-dense.csv` |
| Threshold sweep | `.../raw/16-8-parbench/threshold-sweep.csv` |
| Oracle-relative matrix | `.../raw/16-8-oracle/par-oracle-matrix.csv` + `summary.json` |
| Differential court | `.../raw/16-8-differential/console.log` (+ per-cell stdout/stderr + sha256) |
| Atlas rows | `courts/receipts/phase-16/matrices/16-8-rayon.json` |

---

## 1. §16.8.1 — private pool, never the host's global Rayon pool

`src/xml/parser/scan/pool.rs` builds a **private** `rayon::ThreadPool`, lazily,
in a `OnceLock`, on first eligible use, and routes every parallel operation
through `pool.install(...)`. The global Rayon pool is never constructed, never
queried, and never reconfigured — the library is loaded into Python/lxml, PHP,
Nokogiri and native applications and must not couple its parallelism to a pool
it does not own.

Diagnostic controls (read once per process; explicitly **not** ABI):

| Variable | Values | Meaning |
|---|---|---|
| `LIBXML_RS_PARALLEL` | `off` \| `on` \| `auto` | force sequential / force parallel / threshold-gated (default) |
| `LIBXML_RS_THREADS` | `N` | private-pool worker count (default: `available_parallelism()`; `1` ⇒ no pool) |
| `LIBXML_RS_PARALLEL_THRESHOLD` | bytes | override the measured per-run probe cap |

`Config::engages(len)` gates the whole-input prepass; `Config::per_call_engages`
is deliberately `false` under `auto` (the per-call ordered-wave dispatch is only
amortised when it replaces a long per-character scan, which the whole-input
index replaces instead — see §6). Unit tests cover the mode/threshold logic and
the no-pool fallback.

## 2. §16.8.2 / §16.8.6 — the parallel structural scan and its chunking

The safe first parallel operation is exactly what §16.8.2 asks for: classify
**independent byte blocks** and then reconcile their starting state. Two
mechanisms implement it.

### 2.1 `StructIndex` — whole-input structural index

`parallel::StructIndex` performs one prepass over the base input in
`par_chunks(COARSE = 4096)`. Each coarse block stores, for each of the four
content classes (text, comment, CDATA, PI), the block-local offset of the first
terminator (`u16::MAX` = none). The table is built from a single indexed load
per byte (`TERM_TABLE: [u8; 256]` — bit *c* set iff the byte terminates class
*c*), so the prepass runs at **23.9 GB/s** (2.81 ms for 64 MiB), whereas the
per-character comment/CDATA classifier runs at ~2.4 GB/s.

The classes are **context-free**: a byte terminates a run regardless of what
precedes it. The "compact prefix/state reconciliation so each block receives the
correct starting lexical state" is therefore the identity for this classifier,
which the module documents in detail and the differential court pins. The
tokenizer only ever invokes a class scan in one lexical state and never
re-interprets a skipped byte, so no cross-block quote/comment state is needed.
`structural_summary` additionally computes the §16.8.2 block masks (terminator,
non-ASCII, line-break counts) with a real cross-block prefix accumulation and is
differentially tested against a scalar count.

Chunking (§16.8.6): `par_chunks` gives uniform, **indexed** blocks; each worker
writes its own result into the collected vector; there is **no `par_bridge`**,
no global lock, and no shared mutable accumulator. The per-call ordered-wave
scanner (`first_match_with`) scans a sequential one-`BLOCK` prefix first (so the
common short-run case dispatches nothing) and then `par_chunks(BLOCK)` in ordered
waves (`WAVE_MULT`), returning the first hit in index order.

### 2.2 Tokenizer escalation — the per-run probe

`XmlTokenizer::class_content_run` resolves a comment/CDATA run as follows.

1. If the index exists, `run_len_indexed` answers it. Because a cursor can sit
   past its coarse block's first terminator, the resolver scans **only the
   remainder of the current coarse block** (≤ 4 KiB) and then resumes the index —
   never returning a later block's offset for an earlier answer.
2. Otherwise a bounded probe (`probe_class_run`) scans the run up to
   `budget = clamp(base_len / INDEX_BREAKEVEN_RATIO(11), BLOCK, threshold)`.
   - run ends inside the probe ⇒ exact answer, no pool dispatch, no prepass;
   - probe fully clean ⇒ run reached the measured break-even ⇒ build the index
     **once** and re-resolve.
3. `note_class_run` implements the **many-medium-run** escalation: once the
   cumulative class-run bytes reach `max(base_len/8, 2·BLOCK)`, build the index
   even though no single run reached the crossover.

The `11` is the measured `S/I` ratio (sequential class scan 2.4 GB/s ÷ prepass
24 GB/s ≈ 1/10.9): the prepass is worth `base_len/I` once the remaining
class-run work exceeds `(S/I)·base_len`.

The design deliberately avoids an early "scan the run in parallel, then build the
index and re-scan it" double pass: the probe scans at most one budget, and the
index replaces every later run.

## 3. §16.8.3 — stage progression

| Level | Scope | Status | Evidence |
|---|---|---|---|
| **A** | parallel structural scanning only | **implemented** (this receipt) | `StructIndex`, `structural_summary`, differential court |
| **B** | parallel UTF-8 validation + structural scanning | **implemented** | `parallel::utf8_is_valid_lax` + `xmlCheckUTF8` delegation |
| **C** | parallel lexical extraction after resolving global start state | **not pursued — documented** | §3.3 |
| **D** | parallel subtree/materialization | **not pursued — documented** | §3.4 |

### 3.1 Level A

`StructIndex` + the ordered-wave class scanner. Differential court G1/G2 PASS
(§8).

### 3.2 Level B — parallel UTF-8 validation

`parallel::utf8_is_valid_lax[_with]` reproduces the exact upstream
`xmlCheckUTF8` **shape** semantics (a lead byte `110xxxxx`/`1110xxxx`/`11110xxx`
must be followed by `10xxxxxx` continuations; any other high byte is invalid;
upstream does not reject overlong forms, surrogates or out-of-range code points
here, and neither does this). Blocks are delimited by **lead-byte positions
only**: starting from each block-boundary candidate the reconciler walks forward
over at most three continuation bytes, so a character can never straddle a
block, and a truncated character at a block end is invalid exactly as the
scalar walk sees a non-continuation byte there. The validator is first-class
function `check_utf8` in `src/xml/string.rs` measures the NUL-terminated length
(`strlen`) and dispatches to it; the former byte-at-a-time loop is gone.
`utf8_validator_matches_scalar` (600 deterministic block-spanning buffers) and
`utf8_truncated_at_block_boundary` (every block-offset × character-length ×
truncation combination) pin it against an independent scalar reference.

The parser's own text path keeps its per-character decode: the §16.7.3 error
contract (incomplete vs encoding-error diagnostics, per-byte) depends on the
per-byte sequence, and pre-validating the whole input would duplicate work the
scanner already does only at the (rare) non-ASCII bytes. `long_text` is flat
(§7), so this is a non-issue for the measured surfaces.

### 3.3 Level C — decision

Parallel pure lexical extraction would require globally resolving quote/entity/
namespace start state for arbitrary fragments and then emitting tokens out of
document order (or buffering and re-serialising them). The measured scan-bound
surfaces (comment/CDATA runs) are already fully captured by Level A at 1.5–3.4×,
while the tokenizer-bound surface (`dense`, 62 MB/s) is limited by DOM
construction, not by scanning, so parallel lexical extraction cannot move it.
Pursued only if a future consumer shows a scan-bound profile not covered here.

### 3.4 Level D — decision

Not attempted, per the spec's explicit instruction. DOM construction carries
allocator contention, dictionary interning, parent/child linking, entity and
namespace state, line/error state, user hooks and SAX callbacks; there is no
evidence any of it is safely parallelisable at a worthwhile gain, and an
incorrect parallel parse is failure.

## 4. §16.8.4 — eligibility

`XmlTokenizer::scan_index_eligible` is the conservative gate:

- **base input only** — the index describes exactly the base document byte
  range; a pushed entity-content input is a different buffer with its own
  positions and never uses it;
- **not incremental/push** — in push mode (`xmlParseChunk`) the base buffer is
  refilled and grown across calls, so an index over a partial buffer would be
  stale. Push parsing therefore falls back to the per-call scanners.

Everything else in the §16.8.4 list is satisfied *by construction*: the parallel
scan is a semantics-preserving acceleration (differential-proven byte-identical),
so it needs no gate on the consumer, parser options, SAX/DOM delivery mode, or
external-entity behaviour — and it never influences when a callback is
delivered. Anything outside the eligible surface falls back to the exact
sequential scanner; a fallback is not failure, an incorrect parallel parse is.

## 5. §16.8.5 — SAX callback ordering

SAX callback delivery is never parallelised. The only parallel work is the byte
classification that precedes delivery, and the push path cannot build the index
(§4), so it cannot change across `xmlParseChunk` calls. The pull path scans the
same bytes faster; token order and event order are untouched. The §16.7.8
pushdiff court (`courts/suites/phase16/pushdiff-*`) is the harness that pins
per-call push traces oracle-vs-candidate; the §16.8 differential court pins the
pull side across `off/on/auto`, thread counts, and a threshold that forces the
prepass on nearly every document.

## 6. §16.8.7 — threshold discovery

Method: `examples/parbench.rs` (deterministic in-memory shapes, one child
process per cell because the resolved config is a `OnceLock`, best-of-N per
cell) and `tools/bench/parbench-sweep.sh`. Host: AMD Ryzen 7 9800X3D (8C/16T,
96 MB L3), 16 logical CPUs. Provider isolation is respected: this harness is
candidate-only (single provider).

Isolated scanner (`timing_probe`, 64 MiB clean buffer, AVX2 backend):

```
seq  0.94 ms (71.5 GB/s)
par2  0.95 ms  (0.95×)   ← dispatch + synchronisation not yet amortised
par4  0.57 ms  (1.65×)
par8  0.42 ms  (2.23×)
par16 0.33 ms  (2.86×)   ← memory-bandwidth saturation (205 GB/s)
index build 64 MiB: 2.81 ms (23.9 GB/s)
```

Threshold sweep (`raw/16-8-parbench/threshold-sweep.csv`, best-of-2, 8 threads,
speedup vs 1-thread `off`): with the adaptive, input-proportional probe budget,
the cap is what matters. Representative 64 MiB single-run cells:

| cap | one_comment 64 MiB | one_cdata 64 MiB | many_comment 64 MiB | many_cdata 64 MiB |
|---|---|---|---|---|
| 256 KiB | 1.81× | 3.11× | 1.63× | 1.78× |
| 1 MiB | 1.93× | 2.54× | 1.45× | 1.57× |
| 4 MiB | 1.80× | 2.09× | 1.42× | 1.50× |

**Decision: `DEFAULT_THRESHOLD = 256 KiB`** — the best or statistically tied cap
across the matrix, never below 1.0×. It is a **cap**, not a fixed budget:
`budget = clamp(len/11, 64 KiB, 256 KiB)`, so small documents probe only one
block.

Two non-regression cases pin the choice (both re-measured after the sweep):

- a lone 100 KiB comment in a 64 MiB dense document must **not** build a 64 MiB
  prepass (a 64 KiB cap measured a 12% regression; 256 KiB finds the terminator
  inside the probe and does not build) — `dense_comment` in the matrix is
  1.00–1.07×;
- a 256 KiB all-comment document is a **tie** (1.00×, best-of-12: 76.15 µs
  sequential vs 75.91 µs auto), because `engages` requires at least the
  threshold of input.

## 7. §16.8.8 — scaling metrics

### 7.1 Speedup vs the one-thread optimized candidate

`raw/16-8-parbench/matrix.csv` (best-of-3 children × best-of-5 iterations;
`off` = one-thread sequential):

| shape | 256 KiB | 1 MiB | 4 MiB | 16 MiB | 64 MiB |
|---|---|---|---|---|---|
| one_comment | 1.35× | 2.05× | 2.43× | 2.74× | 1.75× |
| many_comment | 1.00× | 1.49× | 1.68× | 1.57× | 1.67× |
| one_cdata | 1.24× | 1.67× | 2.86× | 3.38× | 2.87× |
| many_cdata | 1.05× | 1.59× | 1.80× | 1.99× | 1.95× |
| long_text | 0.97× | 1.00× | 1.11× | 1.07× | (noise-bound) |
| dense_comment | 1.04× | 1.07× | 1.03× | 1.00× | — |
| dense (tokenizer-bound) | 0.98–1.02× | 1.02× | 1.00× | 1.01× | 1.02× |

Thread count at the measured optimum is 8–16 on this host. `dense` is flat
because it is DOM-construction-bound, not scan-bound: the parallel front end is
correctly *not* engaged where it cannot help.

### 7.2 Speedup vs the oracle (§16.8.8)

`raw/16-8-oracle/par-oracle-matrix.csv`, oracle libxml2 2.15.3 (median of 7
trials, `taskset 0-15`, alternating provider order, separate processes per
provider per atlas §2). `speedup = oracle_time / candidate_time`:

| operation | 1 MiB | 4 MiB | 16 MiB | 64 MiB |
|---|---|---|---|---|
| `parse_comment_many` | 2.18× | 2.26× | 2.13× | 2.70× |
| `parse_comment_one` | 1.35× | 1.99× | — | — |
| `parse_cdata_many` | 15.9× | 17.4× | 16.9× | 19.5× |
| `parse_cdata_one` | 24.7× | 30.1× | — | — |
| `parse_e2e` (dense) | 0.35× | 0.45× | 0.44× | 0.41× |

(Values are the 8-thread `auto` column; `—` = omitted because upstream
`XML_MAX_TEXT_LENGTH` rejects a single >10 MB run, see §10.)

The candidate now leads the oracle on every scan-bound shape, by 1.35×–30×. The
`parse_e2e` (dense markup) row is a **pre-existing, out-of-§16.8 tokenizer-bound
gap** (DOM construction), not a parallel-blocking result; it is recorded here so
it is not hidden.

### 7.3 Parallel efficiency, cycles/byte, RSS

`matrix.csv` carries `efficiency = speedup/threads`, `cycles_per_byte` (at the
9800X3D sustained 4.8 GHz) and `rss_kib` per cell. Representative 64 MiB
`auto,16` rows:

| shape | MB/s | speedup | efficiency | cycles/byte | RSS KiB |
|---|---|---|---|---|---|
| one_comment | 4415 | 1.75× | 0.11 | 1.09 | ~17 MiB |
| many_comment | 3716 | 1.67× | 0.10 | 1.23 | ~17 MiB |
| one_cdata | 9382 | 2.87× | 0.18 | 0.54 | ~17 MiB |
| many_cdata | 4820 | 1.95× | 0.12 | 0.98 | ~24 MiB |

Efficiency falls with thread count exactly as memory bandwidth saturates (the
isolated text scan reaches 205 GB/s at 16 threads), but **throughput does not
regress** past 8 threads on any measured cell — 16 is at or within noise of the
optimum — so `auto` uses `available_parallelism()`. The per-call ordered-wave
path (only reachable under `LIBXML_RS_PARALLEL=on`) peaks near 8 threads and is
where saturation turns into a regression; `auto` never uses it. Auto-selection
therefore stops at the measured optimum: the whole-input prepass.

## 8. Differential court (§16.8.4/§16.8.5 gate)

`courts/suites/phase16/par-differential.sh` (+ `par-run.sh`), inside the oracle
court image, over 100 generated boundary documents
(`gen_par_corpus.py`: comment/CDATA runs at `COARSE`, `BLOCK` and `threshold`
boundaries ±1/±2, planted `-`/`]` mid-run, many-run documents, non-ASCII UTF-8
text runs, a 4 MiB dense document with a lone 100 KiB comment, and a mixed
2 MiB document) plus the phase-14 consumer fixtures.

```
corpus: <N> documents
G1 PASS backend-invariance ref==avx2
G1 PASS backend-invariance ref==avx512
G2 PASS parallel-invariance ref==auto_t1  … auto_t2, auto_t4, auto_t8, auto_t16
G2 PASS parallel-invariance ref==on_t1    … on_t2, on_t4, on_t8, on_t16
G2 PASS parallel-invariance ref==auto_thr0_t2, _t8, _t16   (threshold=1: prepass on nearly every doc)
```

Every configuration is byte-identical to the fully sequential reference in
**both** stdout (parse trees) and stderr (diagnostics): 162 479 node rows,
49 diagnostic lines, identical sha256 across all 16 configurations. The
`auto_thr0_*` cells force the whole-input prepass on almost every document so
the parallel path is exercised far beyond the production threshold.

## 9. §16.4 discipline — before / patch / after / gates / delta

- **before**: the one-thread optimized candidate (`PARALLEL=off`), i.e. the
  §16.5/§16.7 scanners with no parallelism. The measured hypothesis was that the
  per-character comment/CDATA classifier (~2.4 GB/s) is the scan-bound cost for
  large comment/CDATA documents, and that it is a function of a context-free
  byte class whose block summaries reconcile trivially.
- **patch**: `scan/pool.rs` (private pool + controls), `scan/parallel.rs`
  (structural index, probe, ordered-wave scanner, Level B validator),
  `tokenizer.rs` (lazy escalation, eligibility, cumulative rule), `string.rs` +
  `exports_xml2.rs` (Level B `xmlCheckUTF8`), the parbench harness, the corpus
  generator and the differential/oracle courts.
- **after**: §7.1/§7.2. The scan-bound surfaces move 1.49×–3.38× vs the
  one-thread candidate and 2.1×–30× vs the oracle; unhelpful surfaces stay flat.
- **semantic gates**: see §11.
- **delta**: kept on evidence. The rejected variants are recorded in the
  transcript and summarised here: eager index build at parse start (regressed
  dense/text documents), naive coarse-index lookup (wrong answers, caught by
  `struct_index_matches_scalar`), per-character prepass (10 GB/s, fixed by the
  `TERM_MASK` table), and per-call pool dispatch under `auto` (loses on dense
  markup). The final design's probe budget and cumulative floor are both
  measured, not chosen.

## 10. Residuals found (recorded, out of §16.8 scope)

### 10.1 Oversized comment / CDATA runs are accepted

While building the oracle matrix, a **pre-existing** drop-in divergence was
confirmed and is recorded here rather than hidden:

> A single comment or CDATA run longer than `XML_MAX_TEXT_LENGTH` (10 000 000)
> is rejected by upstream (`Comment too big found` / `CData section too big
> found`) but is **accepted** by the candidate.

Reproduced with `tools/bench/harness.c` at 11 MiB:

```
oracle   parse_comment_one 11 MiB → "Entity: line 1: parser error : Comment too big found"
oracle   parse_cdata_one   11 MiB → "Entity: line 1: parser error : CData section too big found"
candidate (both)                  → no diagnostic
```

This is not caused by §16.8 (the pre-parallel per-character loop also accepted
them) and is not exercised by lxml (which is green). It is an oracle-pinned
parity task in its own right (exact code, message, position and recovery) and is
tracked as the next candidate for the §16.5 semantic-parity series, not fixed
inside this performance subphase where it would be un-pinned. The oracle matrix
omits single-run shapes above 4 MiB for this reason.

### 10.2 DTD entity-declaration ownership leak (pre-existing)

After fixing the two bugs in §11, the coverage-guided `parse` fuzzer found a
remaining **pre-existing** ASan leak on
`<!DOCTYPE a [<!ENTITY a>` (a truncated entity declaration):

```
Direct leak of 146 byte(s) in 2 object(s)
  #3 new_entity        src/xml/tree/mod.rs
  #7 fire_entity_decl  src/xml/parser/state.rs
```

The default SAX `entityDecl` handler calls `tree::new_entity`, which only
allocates and fills the entity — its comment claims it also registers it, but it
has no document/DTD argument and does not. On the normal path
the parser registers the entity through `parse_entity_decl`; the handler's
detached allocation is never adopted, so it leaks. Reconciling the two
registration paths changes DTD entity ownership, so it is recorded here (with
the exact input and stack) as the next Phase-16 fuzz-owned task rather than
patched un-pinned inside §16.8. The CI fuzz job is `continue-on-error`.

## 11. Bugs found and fixed by the §16.8 fuzz smoke

Two real defects surfaced while running the fuzz gate for this phase and were
fixed here because both are behaviour/DoS bugs, not performance work.

### 11.1 XML-declaration drain loop hangs on a truncated multibyte byte

Input `3c 3f 78 6d 6c 09 f0` (`<?xml\t\xf0`) hung `xmlReadMemory` forever (the
`parse` fuzzer's timeout). `scan_xml_decl_rest`'s `?>`-drain loop terminates on
`RAW == 0`, but `decl_advance` (`NEXT`) calls `read_char`, which declines to
advance an incomplete multibyte sequence, so `RAW` stayed `0xf0` forever.
Upstream `NEXT` (`xmlNextChar`) never fails to make progress. Fix:
`decl_advance` force-consumes one raw byte when `read_char` did not move the
cursor. After the fix the output is byte-identical to the oracle ("Malformed
declaration expecting version" + "Blank needed here") for `\xf0`/`\xe2`/`\xc3`/
`\xff` tails. Regression test
`xml::parser::tests::test_xml_decl_truncated_multibyte_terminates`.

### 11.2 `ctxt->intSubName` leaked on every DOCTYPE

`<!DOCTYPE a\xbc` leaked the context-owned `intSubName` string. `free_parser_ctxt`
(and `xmlCtxtReset`) reclaimed `version`/`encoding`/`directory` and the external
id pair `extSubURI`/`extSubSystem`, but not `intSubName`, which `parse_doctype_decl`
allocates with the same ownership. Fix: free and null `intSubName` alongside its
siblings in both teardown paths. (A later fuzz input exposed §10.2, a separate
DTD entity leak.)

Neither fix changed any differential result: the §16.8 court's tree and
diagnostic hashes and the §16.7.7 court's candidate stderr hash are identical
before and after.

## 12. Gates

| Gate | Result |
|---|---|
| `cargo test --lib` | 1346 passed / 0 failed / 4 ignored |
| `cargo fmt --check` | clean |
| §16.8 differential court (`par-run.sh`, 175 docs) | G1 PASS ×2, G2 PASS ×13 (stdout+stderr byte-identical across 16 configs) |
| §16.7 differential court (re-run) | G1 PASS; G2 trees byte-identical; candidate stderr hash unchanged |
| §16.7.8 pushdiff court | ordering preserved; push path never builds the index (§4/§5) |
| PHP six-gate (`cand-six-gate.sh`) | 1250 passed / 0 failed / 40 skipped |
| lxml full suite | Ran 2007 tests — OK (0 failures) |
| fuzz parse / html / xpath smoke | html + xpath clean; parse: timeout fixed (§11.1); known-leak residuals §10.2 |
| `tools/courts/regression.sh` | PASSED |

## 13. Atlas rows

`courts/receipts/phase-16/matrices/16-8-rayon.json` carries the 80
oracle-relative rows (all required `PERFORMANCE_ATLAS.md` schema fields;
provider `candidate-rayon`/`candidate-avx2`/`oracle`, backend
`rayon`/`avx2`/`auto-cpu`, per-cell `thread_count`, `speedup =
oracle_median/candidate_median`, RSS). The self-relative matrix is the raw CSV
plus §7.1.
