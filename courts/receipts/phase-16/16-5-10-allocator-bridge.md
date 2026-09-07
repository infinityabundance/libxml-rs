# Phase 16.5.10 — allocator bridge: cache the slot ADDRESS, not the hook

Commit: (this commit)
Date: 2026-09-07
Phase: 16.5.10

## Change

Every internal allocation goes through `xmlMallocImpl`/`xmlFreeImpl`/
`xmlReallocImpl`, which resolve the process-visible allocator hook via the
R-000177 slot bridge: a `OnceLock`-cached dlsym'd accessor
(`__xmlMalloc()` etc.) was INVOKED on every single allocation to fetch the
slot pointer, then the slot was dereferenced.

Per the phase rule — DO NOT cache the current malloc FUNCTION value (it is
reassigned by `xmlMemSetup` / direct `xmlMalloc = ...` assignments and
every allocation must observe the current hook); caching the POINTER TO THE
MUTABLE SLOT preserves mutation semantics — the bridge now runs the dlsym'd
accessor exactly ONCE (lazily, at first allocation) to learn the slot
ADDRESS and caches that address (usize storage keeps the `OnceLock` Sync/
Send). Each allocation then does one load through the cached address,
removing the per-allocation accessor call. Single-DSO links (accessor not
exported) are unchanged: the local exported variable is used directly.

## Proof (allocator-hook court)

New `courts/suites/phase14/consumers/alloc-slot-probe.c`, linked against the
whole-archive facade (`libxml2.so.16` → core), exercises exactly the
semantics the phase demands:

1. default alloc + free;
2. `xmlMemSetup` installs counting hooks → `xmlMalloc`+`xmlFree` MUST hit
   the new hooks (mutation observed through the cached address) — PASS;
3. a full `xmlReadMemory` parse under the counting hooks counts its
   allocations (29) — PASS;
4. `xmlMemSetup(NULL, …)` restores defaults → allocations return to the
   default hook — PASS.

In-crate allocator unit tests (14, incl. `test_direct_assignment_coherence`)
also pass.

## Measurement

Criterion `parse` (every tree-node allocation pays this bridge):
**−3.4% (283 B) / −4.1% (2.9 KB) / −5.0% (31 KB) / −4.2% (328 KB)** — all
four sizes improved, stable across the run.

Cumulative §16.5.x parse gain on this microbench since the first §16.5.3
baseline: 7.05 µs → 6.19 µs (283 B), 70.2 µs → 60.7 µs (2.9 KB),
724 µs → 597 µs (31 KB), 7.18 ms → 6.54 ms (328 KB) — ~10–18% total.

## Gates

- `cargo test --lib`: 1264 pass / 0 fail (incl. all 14 allocator tests).
- PHP six-gate: 1250 passed / 0 failures.
- Allocator-hook C probe: PASS (facade path).
