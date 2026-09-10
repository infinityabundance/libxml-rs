# pushdrive step 5b — no-regression full-court run

## What this is

The same pristine-tree full-court run as
[`../pushdrive-step5/`](../pushdrive-step5/README.md), repeated at the step-5b
commit `57b2f05c` (incremental availability scanning). It is the third point in
the same before/after series, and it is the strongest of the three because the
driver commits turn out to be provably inert with respect to the SHIPPED
LIBRARY, not merely to its behaviour.

| | pre `93fd8282` | step 5 `bad79052` | step 5b `57b2f05c` |
|---|---|---|---|
| `candidate_sha` | `93fd8282` | `bad79052` | `57b2f05c` |
| `candidate_libxml2_sha256` | `37d69eb127ebf03a…` | `3a0b487179082a28…` | `3a0b487179082a28…` |
| `cargo_lock_sha256` | `c25c80fa510c020b…` | `c25c80fa510c020b…` | `c25c80fa510c020b…` |
| `generator_sha256` | `98605a3f462384e5…` | `98605a3f462384e5…` | `98605a3f462384e5…` |
| `probe_sha256` | `ab3f1a9325aa5104…` | `ab3f1a9325aa5104…` | `ab3f1a9325aa5104…` |
| `runner_sha256` | `35c41c5f7dedda5b…` | `35c41c5f7dedda5b…` | `35c41c5f7dedda5b…` |
| `image_id` | `sha256:4366b5053d2a…` | `sha256:4366b5053d2a…` | `sha256:4366b5053d2a…` |
| docs / cells | 257 / 4702 | 257 / 4702 | 257 / 4702 |
| diverging cells | 4702 | 4702 | 4702 |

## Result

1. **The court output is byte-identical across all three runs.** Diffing the
   step-5 and step-5b console logs gives zero lines of difference — all 4702
   `DIFF` lines, same order, same `cells=4702 diffs=4702` /
   `4702/4702 cells diverge` summary.
2. **The candidate `.so` is BIT-IDENTICAL between `bad79052` and `57b2f05c`**
   (`3a0b487179082a283591451ee1d37a5779e30bb873a5366586a0bd2189790cf7`). The
   build really ran on both (`Removed 26 files, 110.4MiB total` then
   `Compiling libxml-rs v0.1.0-alpha.50`); the artifact is nevertheless
   unchanged because every one of these changes lives in code the release
   build eliminates as dead — the driver is reached only from
   `pushdrive::tests` and `parse_chunk` does not call it.

The only hash that differs from the pre-run is the pre-run's: `93fd8282` bumped
the crate version to alpha.50, and the version string is embedded in the
library, so that artifact necessarily differs.

## What this proves, and what it does not

**Proves:** step 5b changed nothing observable on the push surface, and nothing
at all in the shipped release artifact. That is the intent of a court-only
foundation commit.

**Does not prove:** anything about the driver relative to libxml2. The full
trace remains uniformly red and stays red until whole contexts flip to the
persistent path. What backs the driver today is `pushdrive::tests`: partition
equivalence against the candidate's own recursive parser, the consumed-prefix
poisoning oracle, the per-step liveness invariant, and the three-shape
complexity curve (flat at 1/2/4/8 MiB, including one 8 MiB start tag — the
quadratic case that would have gone unnoticed without continuation state).
