# Phase 16.7.5 — AVX-512 policy measurement (first evidence)

Commit: (this commit)
Date: 2026-09-07
Host: AMD Ryzen 7 9800X3D (Zen 5, full 512-bit datapath), oracle court VM
(libxml2 2.15.3), taskset cpu 0, 7 alternating-order trials per cell,
auto-calibrated iteration counts (≥ 30 ms/trial), wall ns/iter medians.

Raw: courts/receipts/phase-16/raw/scan-backend-matrix.csv (every trial).

## Results (median wall ns per parse, provider/backend)

parse_e2e (synthetic element-heavy doc from tools/bench/harness.c):

| bytes | oracle | scalar | avx2 | avx512 | avx2/oracle | avx512/avx2 |
|-------|--------|--------|------|--------|------------|------------|
| 1 KiB | 11.5 µs | 25.7 | 26.0 | 26.1 | 2.27 | 1.00 |
| 16 KiB | 130 µs | 344 | 345 | 347 | 2.64 | 1.01 |
| 128 KiB | 1.02 ms | 2.63 | 2.63 | 2.64 | 2.58 | 1.00 |
| 1 MiB | 7.87 ms | 20.5 | 20.4 | 20.6 | 2.59 | 1.01 |
| 8 MiB | 81.2 ms | 178.9 | 180.2 | 179.6 | 2.22 | 1.00 |

parse_ctx_reuse: the same picture (avx2/oracle 2.3–2.7×, avx512/avx2
0.99–1.01).

## Interpretation (§16.7.5 / §16.16)

- On this host and these (element-heavy, text-light) synthetic documents the
  three scan backends are indistinguishable end-to-end: avx2/scalar
  0.99–1.01×, avx512/avx2 0.997–1.011× — all inside run-to-run noise. The
  text-run classifier is a small fraction of total parse time here; the
  scan's own microbenchmark (single huge text run, module-commit receipt)
  still shows scalar 0.020 s vs avx2/avx512 0.013 s (~35 %).
- The dominant cost is the element/DOM machinery: the candidate parse_e2e
  trails the oracle 2.2–2.6× at every size — the §16.8+ optimization
  target, not the scanner.
- **Policy**: `auto` stays on AVX2. AVX-512 shows no end-to-end advantage at
  any size on Zen 5 for this workload; it remains selectable via the
  explicit LIBXML_RS_SCAN_BACKEND=avx512 override and will be re-measured
  on text-heavy and corpus (§16.10) documents before any auto change. This
  is the evidence-driven outcome §16.7.5/16.16 require — AVX-512 is not
  auto-selected merely because it is wider.

## Next

The crossover question becomes meaningful only on scan-dominated workloads;
the §16.10 real corpus + §16.8 measurement court will re-run this matrix
per corpus category. Note for §16.16: the parse-path gap (2.2-2.6×) must be
attacked before scanner micro-deltas can move the headline.
