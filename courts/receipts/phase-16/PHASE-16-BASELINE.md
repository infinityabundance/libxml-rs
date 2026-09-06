# Phase 16 — Baseline (16.0 first-principle inspection)

**Date:** 2026-09-06
**Candidate git SHA:** `2848ced0a24de843a46e397ed9bfafe7cb208968` (`main`)
**Frozen oracle:** libxml2 **2.15.3** + libxslt **1.1.45**, source-built in the
Phase 14/15 Docker image at `/usr/local`.

This document is the pre-modification snapshot required by §16.0. No code was
changed to produce it. It records the repository, hardware, toolchain, oracle,
existing benchmark harness, and current consumer correctness state.

---

## 1. Candidate identity

| Field | Value |
|---|---|
| SHA | `2848ced0a24de843a46e397ed9bfafe7cb208968` |
| Short SHA | `2848ced0` |
| Branch | `main` |
| Last commit | `gitignore: fuzz/Cargo.lock (detached cargo-fuzz workspace)` |
| Working tree | clean |

---

## 2. Frozen oracle

| Field | Value |
|---|---|
| libxml2 | 2.15.3 |
| libxslt | 1.1.45 |
| libxml2 DSO | `libxml2.so.16.1.3` (SONAME `libxml2.so.16`) |
| libxslt DSO | `libxslt.so.1.1.45` (SONAME `libxslt.so.1`) |
| install prefix | `/usr/local` (inside `phase14-debian:1` image) |
| build script | `oracle/build.sh` |

### Oracle build configuration (libxml2)

```
./configure --prefix=/usr/local --enable-shared --enable-static \
  --with-threads --with-thread-alloc --with-http --with-ftp --with-catalog \
  --with-schemas --with-schematron --with-regexps --with-modules \
  --with-xinclude --with-xpath --with-xptr --with-c14n --with-html \
  --with-reader --with-writer --with-valid --with-relaxng --with-icu \
  --with-iconv --with-zlib --with-lzma --with-python=no \
  --without-debug --without-mem-debug --without-run-debug \
  --with-output --with-sax1 --with-legacy
```

### Oracle build configuration (libxslt)

```
./configure --prefix=/usr/local --enable-shared --enable-static \
  --with-libxml-prefix=/usr/local --with-plugins --without-debug \
  --without-mem-debug --without-crypto --without-python
```

### Oracle compiler

GCC **12.2.0** (Debian 12.2.0-14+deb12u1) — the container toolchain.

**Note on host-vs-container compiler:** the candidate is built on the *host*
with GCC 16.2.1 + rustc 1.98.0/LLVM 22, while the frozen oracle is built
*inside the Debian container* with GCC 12.2.0. This asymmetry is recorded
honestly and is itself a candidate for a fairness-receipt (§16.21) rather than
being silently papered over.

---

## 3. Host environment (where candidate builds / performance court runs)

| Field | Value |
|---|---|
| Rust | `rustc 1.98.0` (commit `88d9e12ae`, host `x86_64-unknown-linux-gnu`) |
| LLVM | 22.1.8 |
| C compiler | `cc (GCC) 16.2.1 20260810` |
| Linker | GNU ld (GNU Binutils) 2.47 |
| libc | glibc 2.44 (`ldd (GNU libc) 2.44`) |
| Kernel | `Linux vanir 7.2.2-1-cachyos x86_64` (PREEMPT_DYNAMIC) |
| OS | CachyOS (Arch-derived) |

### CPU

| Field | Value |
|---|---|
| Model | AMD Ryzen 7 9800X3D 8-Core Processor |
| Vendor | AuthenticAMD (Zen 5) |
| Microcode | `0xb404035` |
| Physical cores | 8 |
| Logical CPUs | 16 |
| Sockets | 1 |
| SMT | enabled (Threads per core: 2) |
| NUMA nodes | 1 (node 0, cpus 0–15) |
| L1d | 384 KiB (8 × 48 KiB) |
| L1i | 256 KiB (8 × 32 KiB) |
| L2 | 8 MiB (8 × 1 MiB) |
| L3 | 96 MiB (1 instance, 3D V-Cache) |
| Scaling driver | `amd-pstate-epp` |
| Governor | `performance` |
| Boost/CPB | enabled (`/sys/.../cpufreq/boost` = 1) |
| Max frequency | 5,271,622 kHz |

### SIMD capability (from `/proc/cpuinfo` flags)

AVX, AVX2, and the full AVX-512 family are present on this CPU:

```
avx avx2 avx512f avx512dq avx512ifma avx512cd avx512bw avx512vl
avx_vnni avx512_bf16 avx512vbmi avx512_vbmi2 avx512_vnni avx512_bitalg
avx512_vpopcntdq avx512_vp2intersect
```

This is an AMD Zen 5 (AVX-512-capable) host, not an AVX2-only or Intel
AVX-512 generation. §16.18 requires recording that the policy established here
is for *this* microarchitecture and must not be extrapolated to all x86-64.

### Memory

| Field | Value |
|---|---|
| Total | 125 GiB |
| NUMA node 0 size | 128,450 MB |
| Swap | 125 GiB |

### GPU / CUDA

| Field | Value |
|---|---|
| GPU | NVIDIA GeForce RTX 4080 SUPER (AD103, rev a1) |
| VRAM | 16,376 MiB |
| Driver | NVIDIA 610.57.04 (KMD), CUDA UMD 13.3 |
| `nvidia-smi` | present |
| `nvcc` | **not installed** (no CUDA toolkit) |
| CUDA runtime API | driver present; toolkit absent → driver-API (dynamically resolved) is the only immediately-available path |

---

## 4. Storage

| Field | Value |
|---|---|
| Project disk | `nvme1n1` KINGSTON SNV2S2000G (1.8 TB NVMe) |
| Mount | `/mnt/1tb_kingston` (1.3 T, 227 G free, 83% used) |
| Secondary NVMe | `nvme0n1` CT2000T705SSD3 (1.8 TB) |

---

## 5. Candidate build configuration

From `Cargo.toml`:

```toml
[profile.release]
lto = true
codegen-units = 1
panic = "abort"
strip = "symbols"

[profile.dev]
debug = true
overflow-checks = true

[profile.bench]
panic = "unwind"   # criterion needs unwind; release cdylib keeps abort
```

Custom linker via `.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "tools/packaging/linker-wrapper.sh"
```

The linker wrapper passes through to `cc`, scopes the version-script rewrite to
the core `liblibxml_rs*.so` cdylib, and regenerates the libxslt/libexslt facade
DSOs + versioned profile post-link.

**Candidate production (release) artifacts present at baseline:**

- `target/release/liblibxml_rs.so`
- `target/release/lib/libxslt.so.1.1.45`
- `target/release/lib/libexslt.so.0.8.25`
- (versioned `libxml2.so.2` profile is **not** present in `target/release/versioned` at baseline; it is generated on demand from the distro DSO.)

---

## 6. Existing performance harness (pre-Phase-16)

| Artifact | Purpose | Status |
|---|---|---|
| `benches/benchmarks.rs` | Criterion microbench (parse/xpath/serialize/xslt/html, size Pareto) | present, compiles |
| `tools/bench/harness.c` | single C source compiled against oracle & candidate, CSV out | present |
| `tools/bench/pareto_matrix.py` | compiles harness against both prefixes, emits `matrix.json`/`.md` | present |
| `tools/bench/run_in_docker.sh` | runs the matrix in `phase14-debian:1` image | present |

**Known limitations of the pre-Phase-16 harness (to be evolved per §16.2):**

- Fixed small iteration counts, no auto-calibration to a minimum duration.
- Oracle typically runs before candidate (no A/B/B/A alternation / balanced blocks).
- No CPU pinning, no thermal/frequency capture, no confidence intervals, no raw sample retention.
- Aggregates are latency/throughput point estimates, not a statistical court.

### Baseline Pareto matrix (existing synthetic corpus, as last run)

Candidate is currently **slower than the oracle on every synthetic op** (the
honest starting point — no manufactured wins):

| op | input | oracle | candidate | speedup |
|---|---|---|---|---|
| parse | 1 KiB | 14.99 µs | 37.78 µs | 0.397× |
| parse | 16 KiB | 142.40 µs | 511.23 µs | 0.279× |
| parse | 128 KiB | 1.163 ms | 4.355 ms | 0.267× |
| xpath | 16 KiB | 166.25 µs | 1.319 ms | 0.126× |
| html | 16 KiB | 266.03 µs | 420.07 µs | 0.633× |
| serialize | 16 KiB | 176.39 µs | 558.43 µs | 0.316× |
| validate | 16 KiB | 143.57 µs | 532.00 µs | 0.270× |
| xslt | 1 KiB | 39.33 µs | 95.28 µs | 0.413× |

Raw machine-readable copy: `target/bench-matrix/matrix.json` (and `matrix.md`).

---

## 7. Hot-path source surfaces (§16.0 inspection list)

Line counts at baseline (candidate engine):

| Surface | Lines |
|---|---|
| `src/xml/parser/state.rs` | 6,592 |
| `src/xml/parser/tokenizer.rs` | 2,166 |
| `src/xml/parser/input.rs` | 2,024 |
| `src/xml/parser/helpers.rs` | 1,207 |
| `src/xml/html/mod.rs` | 4,212 |
| `src/xml/tree/mod.rs` | 7,094 |
| `src/xml/xpath/exports.rs` | 5,284 |
| `src/xml/xpath/eval.rs` | 1,110 |
| `src/xml/xpath/context.rs` | 1,353 |
| `src/xml/xpath/axes.rs` | 772 |
| `src/xml/xpath/functions.rs` | 685 |
| `src/abi/allocator.rs` | 1,799 |
| `src/xslt/` (whole) | ~17,769 |

Specific §16.5 hotspots flagged for first inspection:

- `src/xml/parser/state.rs` — parser global side-state / cleanup path (§16.5.1).
- `src/xml/parser/input.rs` — `xmlReadMemory` input ownership / `to_vec` copy (§16.5.2).
- `src/xml/parser/tokenizer.rs` — token payload materialization (`Vec<u8>`) (§16.5.3).
- `src/xml/html/mod.rs` — `html_tag_lookup` lowercasing + linear scan, `auto_close_element` dead `open_names` (§16.5.7/16.5.8).
- `src/xml/xpath/context.rs` / `functions.rs` — core-function `HashMap` rebuilt per context (§16.5.9).
- `src/abi/allocator.rs` — cross-DSO allocator-hook accessor (§16.5.10).

These are hypotheses to be *verified against current HEAD*, not assertions of
current behavior (the prompt snapshot may have drifted).

---

## 8. Correctness gates a parser-engine change can affect (§16.0)

- `cargo test --lib` — **1,258 passed, 0 failed, 1 ignored** at baseline.
- ABI differential courts (`courts/suites/data-abi/*`).
- Allocator courts (after allocator changes).
- Threading courts (after Rayon changes).
- Callback/SAX courts (after parser changes).
- Hostile/failure courts.
- Five-consumer gates (below).

### Five-consumer correctness status at baseline (carried from Phase 14/15)

| Consumer | Status |
|---|---|
| **php** | 0 failures (Phase 14 sealed, six-extension NTS+ZTS). Must re-gate after shared-engine change. |
| **lxml** | ~164 candidate-driven failures at Phase 15.1 baseline; `test_xslt` reduced 51 → 18 this session. Not sealed. |
| **nokogiri** | 2 SIGSEGV crash sites (XPath extension-function `c_context->doc`; SAX empty-PI) + HTML5/CSS failures. Not sealed. |
| **debian reverse-deps** | 0 missing symbols after Phase 15; 5 reverse-deps load & run. Runtime smoke pending. |
| **bind9** | not started. |

Phase 16 must **not** declare any performance claim "sealed" through a consumer
whose correctness path is still broken (§16.23).

---

## 9. Benchmark environment variables (recorded at baseline)

- `CFLAGS="-O1 -g0"` (consumer builds via `courts/suites/phase14/consumers/lib.sh`).
- `LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, `PYTHONIOENCODING=utf-8`, `PYTHONUTF8=1`.
- No `LIBXML_RS_*` backend/thread override variables exist yet (to be introduced in Phase 16).

---

## 10. Phase 16 execution sequence (reference)

16.0 baseline (this doc) → 16.1 forensic parity surface → 16.2 Pareto rebuild →
16.3 operation decomposition → 16.4 profiling discipline → 16.5 scalar work
removal → 16.6 scalar engine → 16.7 SIMD → 16.8 Rayon → 16.9 CUDA →
16.10 100-file corpus → 16.11 eligibility freeze → 16.12–16.13 five-consumer
suites + predeclared queries → 16.14 result equivalence → 16.15–16.22 matrices,
dispatch, regression court, microarch, memory, cache, build fairness, PGO →
16.23–16.29 compatibility, concurrency, fuzzing, oracle-through-consumers,
aggregation, claim language → 16.30–16.33 artifacts + mandatory experiments →
16.34 final seal.
