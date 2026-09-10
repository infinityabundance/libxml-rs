# pushdrive step 6b — no-regression full-court run

## What this is

The same pristine-tree full-court run as
[`../pushdrive-step5b/`](../pushdrive-step5b/README.md), repeated at
`8a3e438b` (encoder error ingress + EOF flush + the shadow-court harness
changes, which include an opt-in modifier to the shared probe).

Its purpose is narrower than the earlier runs': the step-5b commits touched no
code the release build keeps, so the artifact was bit-identical. This one adds
reachable-looking surface (`XmlParser::finalize_end_document`,
`push_persistent`'s ingress guard, `helpers::raise_invalid_encoding` becoming
`pub(crate)`) AND edits `pushdiff-probe.c`, the instrument the frozen court is
measured with. Both need to be shown inert.

| | step 5b `57b2f05c` | step 6b `8a3e438b` |
|---|---|---|
| docs / cells | 257 / 4702 | 257 / 4702 |
| diverging cells | 4702 | 4702 |
| `candidate_libxml2_sha256` | `3a0b487179082a28…` | `dfb3d725168b6f62…` |
| `probe_sha256` | `ab3f1a9325aa5104…` | `24e70bdb41c49af5…` |
| `cargo_lock_sha256` | `c25c80fa510c020b…` | `c25c80fa510c020b…` (identical) |
| `generator_sha256` | `98605a3f462384e5…` | `98605a3f462384e5…` (identical) |
| `image_id` | `sha256:4366b5053d2a…` | `sha256:4366b5053d2a…` (identical) |

## Result

**The court console is byte-identical to the step-5b run** — `diff` of the two
console logs yields zero lines, all 4702 `DIFF` lines in the same order, the
same `cells=4702 diffs=4702` summary. That is the behavioural claim, and it
covers both changes at once: the driver additions are release-inert, and the
probe's new `i` (inline-final) modifier leaves every pre-existing plan's trace
untouched.

Two hashes did change, neither of them a behaviour claim:

- `probe_sha256` — the modifier is a real source change to the instrument. The
  opt-in parse (`while (*m == 'z' || *m == 'i')`) cannot affect a plan that
  does not end in `i`, and the byte-identical trace confirms it. No plan used
  by `pick_modes` in the differential script contains `i`.
- `candidate_libxml2_sha256` — a new `pub(crate)` method and a guard change
  shift code layout even where behaviour is unchanged. Unlike step 5 → 5b,
  bit-identity is NOT claimed here; only the byte-identical trace is.

## What this proves, and what it does not

**Proves:** the encoder fixes and the probe modifier changed nothing observable
on the replay push surface. `parse_chunk` is still the replay engine, and the
full court is uniformly red exactly as before.

**Does not prove:** anything about the driver relative to libxml2 — that is the
oracle-shadow court's job, which at this commit reports **78 cells, 78 match,
0 diverge** (`cargo test --lib pushshadow -- --ignored --nocapture`).
