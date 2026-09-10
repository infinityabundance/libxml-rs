# pushdrive step 5 — no-regression full-court comparison

## Why this directory exists

Slice 1 step 5 adds the persistent push driver (`src/xml/parser/pushdrive.rs`,
commit `bad79052`) but deliberately does **not** wire it into
`helpers::parse_chunk`. The claim to substantiate is therefore narrow and
falsifiable:

> adding the driver changes nothing about the candidate's push behaviour,

because the driver is reached only from `pushdrive::tests`, and `parse_chunk`
plus the progressive source decoder are byte-for-byte untouched.

An assertion that "nothing changed" is worth nothing without a before/after
measurement, so this directory records the SAME court run at the two commits
either side of the change, over the full (unfiltered) corpus.

Note that the pre-change full-corpus count has never been recorded before: the
frozen slice-0 baseline (4042 cells) was measured against a smaller corpus, and
the decoder slice only ever ran the court **filtered** to the 31 `enc-*`
documents (565 cells). This is the first full-corpus baseline for the current
257-document court.

## The two runs

Both runs are launched from a pristine committed worktree (`tree_clean=yes`),
each by `courts/suites/phase16/pushdiff-run.sh`, which builds the candidate
itself in `target/release` so the recorded binary sha256 chains to the recorded
`candidate_sha`. Both use the identical court image, generator, probe, runner
and `Cargo.lock`.

| | pre (`pre-run.txt`) | post (`post-run.txt`) |
|---|---|---|
| `candidate_sha` | `93fd8282` (alpha.50, publish commit) | `bad79052` (driver commit) |
| `candidate_libxml2_sha256` | `37d69eb127ebf03a…` | `3a0b487179082a28…` |
| `court_sha` | `93fd8282` | `bad79052` |
| `generator_sha256` | `98605a3f462384e5…` | `98605a3f462384e5…` (identical) |
| `cargo_lock_sha256` | `c25c80fa510c020b…` | `c25c80fa510c020b…` (identical) |
| `image_id` | `sha256:4366b5053d2a…` | `sha256:4366b5053d2a…` (identical) |
| `pushdiff_filter` | (empty — full corpus) | (empty — full corpus) |
| docs | 257 | 257 |
| cells | 4702 | 4702 |
| diverging cells | 4702 | 4702 |

## Result

The court output is **byte-identical** between the two runs — all 4702 `DIFF`
lines, in the same order, with the same final `cells=4702 diffs=4702` /
`4702/4702 cells diverge` summary. The only differences anywhere in the two
captures are the provenance fields above: `candidate_sha`, `court_sha` and the
candidate binary sha256 (unavoidable — an unused module still changes the
linked binary), plus the `generator_sha256` note if the corpus generator moved
between the two commits (it does not here).

## What this proves, and what it does not

**Proves:** the persistent-driver commit is behaviour-preserving for the push
surface. Every one of the 4702 cells still diverges exactly as it did before —
the replay engine is untouched, which is precisely the intent of a court-only
foundation commit.

**Does not prove:** anything about correctness of the driver relative to
libxml2. The full trace remains uniformly red and is expected to stay red until
whole contexts are flipped from replay to the persistent path; the driver's own
court (`pushdrive::tests`, partition equivalence + forward-only + prefix
poisoning + liveness + the 8x-linear complexity curve) is what backs it today.
