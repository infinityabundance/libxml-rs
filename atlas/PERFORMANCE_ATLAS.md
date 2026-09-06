# PERFORMANCE_ATLAS — libxml-rs Phase 16

Generated evidence system for performance parity. This is the single
authoritative record of *how* a performance number is produced, classified,
and aggregated. Machine-readable spec lives in `PERFORMANCE_ATLAS.json`.

## 1. Speedup convention (fixed, never reversed)

```
speedup = oracle_time / candidate_time
```

- `> 1.0` → candidate is faster than the frozen oracle.
- `< 1.0` → oracle is faster (candidate regression).
- `== 1.0` → tie.

Every receipt, matrix, consumer table, and aggregate uses this convention.

## 2. Provider isolation

The oracle and the candidate libxml2-compatible providers are **never loaded
into the same address space** for measurement. Loading both risks ELF symbol
interposition, global-state contamination, allocator contamination, callback
contamination, and false measurements.

- Oracle samples come from an oracle-linked harness / consumer process.
- Candidate samples come from a candidate-linked harness / consumer process.
- Criterion remains the primary statistical framework for candidate *internals*
  (single provider), never for the oracle-vs-candidate comparison.

## 3. Canonical benchmark row schema

Every performance observation carries all of the following fields. Missing
fields are an invalid row, not a default.

| Field | Type | Meaning |
|---|---|---|
| `provider` | enum | `oracle` \| `candidate` \| `candidate-scalar` \| `candidate-avx2` \| `candidate-avx512` \| `candidate-rayon` \| `candidate-cuda` \| `candidate-auto` |
| `candidate_sha` | string | full git SHA (empty for oracle) |
| `oracle_identity` | string | e.g. `libxml2 2.15.3 + libxslt 1.1.45` |
| `operation` | string | decomposed op (§16.3), e.g. `xml_parse_only` |
| `consumer` | string | `harness` \| `xmllint` \| `xsltproc` \| `python3-lxml` \| `ruby-nokogiri` \| `php` |
| `corpus_file` | string | manifest stable ID / filename |
| `corpus_category` | string | JATS/SEC/OSM/DBLP/AOSP/Maven/TEI/MusicXML/SVG/DocBook/GPX-KML/RSS-Atom/SOAP-WSDL-XSD |
| `bytes` | int | input byte length |
| `encoding` | string | UTF-8 / UTF-16 / ISO-8859-1 / … |
| `backend` | string | `scalar` \| `avx2` \| `avx512` \| `auto` \| `cuda` \| `rayon` \| `auto-cpu` |
| `simd_isa` | string \| null | actual ISA used (null for oracle/scalar) |
| `thread_count` | int | 1 for single-threaded |
| `cuda_state` | string | `off` \| `forced` \| `auto-selected` \| `not-supported` |
| `cache_mode` | string | `warm` \| `cold` |
| `trial_count` | int | number of independent repeated trials |
| `iterations` | int | iterations per trial (auto-calibrated) |
| `median_ns` | float | median wall latency |
| `mean_ns` | float | mean wall latency |
| `stddev_ns` | float | sample standard deviation |
| `mad_ns` | float | median absolute deviation |
| `p50_ns` | float | |
| `p95_ns` | float | |
| `p99_ns` | float | where meaningful |
| `throughput_bytes_per_sec` | float | `bytes / mean` |
| `confidence_interval` | object | `{level, lower, upper}` on the mean |
| `speedup` | float | `oracle_mean / candidate_mean` (oracle row carries 1.0 baseline) |
| `rss_kib` | int | peak RSS during the trial |
| `cpu_time_ns` | float | CPU time (where distinguishable) |
| `wall_time_ns` | float | wall time |
| `result_hash` | string | deterministic semantic fingerprint (§16.14) |
| `result_valid` | bool | false ⇒ `INVALID_RESULT`, never a speedup |
| `host_fingerprint` | string | hash of the §16.0 host descriptor |

## 4. Statistical classification

A performance "win" is never declared from a bare point estimate above `1.0×`.

| Verdict | Rule |
|---|---|
| `SIGNIFICANT_WIN` | final (99%) CI of speedup lies entirely above `+practical_noise` |
| `PRACTICAL_WIN` | exploratory (95%) CI of speedup lies entirely above `+practical_noise`, but final CI not yet established |
| `TIE` | CI overlaps the practical-noise band `[1/practical_noise, practical_noise]` |
| `PRACTICAL_REGRESSION` | exploratory CI entirely below `1/practical_noise` |
| `SIGNIFICANT_REGRESSION` | final (99%) CI entirely below `1/practical_noise` |

Defaults (recorded in `PERFORMANCE_ATLAS.json`):

- `exploratory_confidence` = `0.95`
- `final_confidence` = `0.99`
- `practical_noise_ratio` = `1.05` (a ratio within ±5% is practically a tie)

A tiny statistically-significant difference is still reported as `TIE` when it
falls inside the practical band. `1.003×` is never described as an improvement.

## 5. Aggregation discipline

- Per-workload speedup → per-category geometric mean → per-consumer geometric
  mean → overall geometric mean (equal-weighted workload/category).
- Bytes-weighted throughput is a **separate** metric, never the headline.
- Speedup ratios are combined with geometric mean, never arithmetic mean.
- A giant file (e.g. DBLP) does not dominate the equal-weighted aggregate.

## 6. Evidence layout

```
courts/receipts/phase-16/raw/        raw trial samples (never deleted)
courts/receipts/phase-16/profiles/   perf/assembly/allocation profiles
courts/receipts/phase-16/matrices/   derived Pareto/consumer matrices
atlas/PERFORMANCE_ATLAS.md           this document
atlas/PERFORMANCE_ATLAS.json         machine-readable schema + thresholds
```

## 7. Regeneration

`PERFORMANCE_ATLAS.json` is the schema of record. All tooling (§16.2 harness,
§16.15 matrix, §16.27 aggregation) must emit rows conforming to this schema and
fail loudly (not silently) on a row that violates it.

Outliers are retained in `raw/`. They are never deleted because they hurt the
candidate.
