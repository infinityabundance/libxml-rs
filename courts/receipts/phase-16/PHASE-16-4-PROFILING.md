# Phase 16.4 — Profiling discipline (tooling + workflow + receipt schema)

**Date:** 2026-09-06
**Candidate SHA:** `3acf0d57` (this receipt was authored immediately after the
§16.3 decomposition; the discipline applies to all subsequent §16.5–§16.22 work).

## 1. Tooling availability (honest inventory)

| Tool | Host | Verdict |
|---|---|---|
| `perf` | not installed | **unavailable** (`sudo` is password-gated; cannot install without operator action) |
| `valgrind` | 3.25.1 | **broken on host**: crashes in `_dl_start` — glibc 2.44's loader emits an EVEX/AVX-512 instruction (`0x62 0xF1 0x7F 0x48 0x7F …`) that LibVEX 3.25 cannot decode |
| `valgrind` (in `debian:bookworm`, glibc 2.36) | 3.19.0 | **usable** — installs and runs; candidate DSO only requires `GLIBC_2.34` ≤ 2.36, so host-built candidate DSOs run in the container |
| `llvm-objdump` / `objdump` | present | **usable** for generated-assembly inspection |
| `cargo llvm-cov` | 0.8.6 | usable for coverage |
| benchmark harness (`tools/bench/harness.c` + `analysis.py`) | present | **primary** wall/CPU/RSS delta measurement |
| `ltrace`/`strace` | absent on host | present in the Debian court container |

## 2. Profiling workflow (before / patch / after / gates / delta)

For every major optimization family (§16.5, §16.7 SIMD, §16.8 Rayon, §16.9 CUDA):

1. **before** — profile the current HEAD on the target hot path:
   - source-level hypothesis + code inspection,
   - `objdump`/`llvm-objdump` generated-assembly confirmation (allocation, branch,
     decode, copy patterns),
   - harness wall/CPU/RSS baseline (the statistical court from §16.2),
   - valgrind callgrind/cachegrind/massif **inside `debian:bookworm`** where a call
     graph is needed (host valgrind is unusable).
2. **patch** — smallest coherent fix.
3. **after** — re-profile the identical surface.
4. **semantic gates** — `cargo test --lib` + relevant differential/ABI/allocator/
   callback courts + consumer re-gates (§16.23).
5. **delta** — record before/after in a `16-4-*.json` receipt; keep or revert on
   evidence.

## 3. Receipt schema (`courts/receipts/phase-16/16-4-*.json`)

```json
{
  "schema": "phase-16-profile-receipt/1",
  "optimization": "string",
  "hypothesis": "string",
  "before": {
    "source_notes": "string",
    "asm_notes": "string",
    "callgrind_top": ["fn → inclusive%", "…"],
    "harness": { "op": "…", "mean_ns": 0, "rss_kib": 0 }
  },
  "patch": "commit-or-description",
  "after": {
    "asm_notes": "string",
    "callgrind_top": ["…"],
    "harness": { "op": "…", "mean_ns": 0, "rss_kib": 0 }
  },
  "semantic_gates": ["cargo test --lib: N passed"],
  "performance_delta": { "speedup": 0.0, "rss_delta_kib": 0, "verdict": "…" }
}
```

## 4. Honest constraint

Because `perf` is unavailable and host `valgrind` is unusable, the primary
causality evidence is **source-level hypothesis + generated-assembly inspection +
harness wall/CPU/RSS deltas**, with valgrind call/cache/massif run in
`debian:bookworm` when a call graph is indispensable. This is a documented
deviation from the ideal `perf record/report/stat` workflow (§16.4 "where
available"); the discipline (before/patch/after/gates/delta) is unchanged.
