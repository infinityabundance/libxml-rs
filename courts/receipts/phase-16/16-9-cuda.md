# Phase 16.9 — CUDA structural accelerator: research receipt

Commit: (this commit)
Date: 2026-09-13
Phase: 16.9 (§16.9.1–§16.9.6). Candidate SHA at measurement: `e525f7e2` + this
change set. Host: AMD Ryzen 7 9800X3D (8C/16T) + **NVIDIA GeForce RTX 4080
SUPER** (Ada, SM 8.9, 80 SMs, 16 GiB), driver **615.71.09**, **PCIe 4.0 x16**,
toolkit `nvcc` 13.3 (used only to pre-generate the embedded PTX).

Raw evidence:

| Artifact | Path |
|---|---|
| Device-code source + provenance | `src/xml/parser/scan/cuda/{struct_scan.cu,struct_scan.ptx,PROVENANCE.md,struct_scan.ptx.sha256}` |
| Transfer / scaling matrix | `courts/receipts/phase-16/raw/16-9-cuda/bench.txt` |
| GPU-vs-CPU DSO differential | `courts/receipts/phase-16/raw/16-9-cuda/{console.log,run.txt,sha256.txt}` |
| Atlas rows | `courts/receipts/phase-16/matrices/16-9-cuda.json` |

**Headline result (§16.9.5): CUDA loses every end-to-end production workload on
this host, so `auto` deliberately stays on the CPU.** The GPU Stage-1 kernel is
~9× faster than the CPU prepass when resident (1.1 ms vs 10.5 ms for 256 MiB),
but the PCIe 4.0 H2D transfer (10–19 ms for the same input) erases the win; the
CPU prepass already runs at ~22–25 GB/s, comparable to the link. This is the
result the spec explicitly names as valid and valuable, and no losing GPU path
is retained in the default.

---

## 1. §16.9.1 — CUDA stays optional

The backend is compiled only under the **`cuda`** Cargo feature (off by
default). Even then it does **not** link CUDA:

- the **driver API** (`libcuda.so.1`) is resolved at runtime with
  `dlopen`/`dlsym` (no `libcuda` link, no `libcudart`, no toolkit);
- the device code is an **embedded PTX string** (`struct_scan.ptx`,
  `include_str!`) JIT-compiled by the installed driver via
  `cuModuleLoadData`;
- the PTX is generated from the reviewable `struct_scan.cu` by
  `tools/cuda/regen-ptx.sh` and checked in with its SHA-256 and the exact
  `nvcc` version (`PROVENANCE.md`).

A normal `cargo build` (feature off) does not compile the module at all —
`select()` is never reached, the device is never initialized, and the artefact
is the same CPU-only drop-in as before. There is no second workspace crate; the
code lives in the existing package under `src/xml/parser/scan/cuda/`.

Verified: default `cargo test --lib` = 1346/0; `cargo build --release --lib`
(feature off) unchanged; feature-on tests build and pass.

## 2. §16.9.2 — what runs on the GPU

Only Stage-1, massively data-parallel structural classification. One warp per
`COARSE` (4 KiB) block produces, for every block:

- `first4[block*4+c]` — the block-local offset of the **first terminator** of
  class `c` (text/comment/CDATA/PI), or `0xFFFF`. This is bit-for-bit the CPU
  `StructIndex` table (`scan::parallel::summarize_block`), so CPU Stage-2
  (`run_len_indexed`) is untouched.
- `stats4[block*4+k]` — per-block counts of text terminators, **bytes ≥ 0x80**
  (ASCII/non-ASCII classification), CR/LF line breaks, and **quote bytes**
  (`"`/`'`, a Stage-2 attribute hint).

No DOM node, pointer, callback, branch-state, or ABI structure is built on the
device. The class predicates are the same `TERM_TABLE` bits the CPU scanner
uses, re-expressed in the kernel. Quote *state* masks belong to CPU Stage-2:
the run classifier is invoked in one lexical state at a time and is stateless
per class, so there is no GPU-side quote scan to do.

Warp intrinsics (`__reduce_min_sync`/`__reduce_add_sync`, lowered to
`redux.sync.min.u32`/`redux.sync.add.s32` in the PTX) reduce each block in one
instruction per class/count.

## 3. §16.9.3 — batching

`State::index_offsets(&[&[u8]])` uploads every source into one device buffer at
`COARSE`-aligned offsets and launches **once** over the union of blocks. No
block straddles two documents (each source's region is padded to a block
boundary; the `ends[]` table clamps each source's trailing partial block), so
each source stays semantically independent — the sources are concatenated as
*transfer descriptors*, never as one XML document. The batch blocks are
asserted equal to the per-document blocks in `cuda_batch_matches_per_document`
and again in the bench.

## 4. §16.9.4 — transfer engineering

`cuda_transfer_bench` (best of 5, warm; `raw/16-9-cuda/bench.txt`):

| input | CPU prepass | GPU **end-to-end** | GPU kernel only | H2D pageable | H2D pinned | D2H |
|---|---|---|---|---|---|---|
| 1 MiB | 50 µs | 137–160 µs | 32 µs | 41–55 µs | 53 µs | 4 µs |
| 4 MiB | 178 µs | 289 µs | 33 µs | 187–192 µs | 170 µs | 5 µs |
| 16 MiB | 625–672 µs | 884–918 µs | 55 µs | 769–788 µs | 640 µs | 7 µs |
| 64 MiB | 2.63–2.69 ms | 4.04–4.07 ms | 152–157 µs | 3.65–3.90 ms | 2.51 ms | 16–18 µs |
| 256 MiB | 10.0–11.2 ms | 20.9–26.5 ms | 1.02–1.14 ms | 18.1–19.0 ms | 10.0 ms | 51–61 µs |
| batch 64 × 1 MiB | 3.25–3.52 ms | 3.98–5.48 ms | 168–172 µs | — | — | — |
| batch 256 × 1 MiB | 12.6–14.8 ms | 21.8–21.9 ms | 1.12 ms | — | — | — |

Headline timing is **transfer-inclusive** (host staging, H2D, launch, kernel,
D2H, reconciliation); `gpu_kernel_ns` is a diagnostic and is never presented as
the speedup. Page-locked (`cuMemHostAlloc`) H2D reaches ~25.6 GB/s vs
~14–16 GB/s pageable; the kernel itself runs at ~230 GB/s; D2H of the compact
table is negligible. The end-to-end `speedup_vs_cpu` is 0.37–0.82× on every
row — **the GPU never wins end to end**.

## 5. §16.9.5 — selection threshold

Control: `LIBXML_RS_ACCEL=cpu|cuda|auto` (default `auto`). `auto` selects CUDA
only above `AUTO_THRESHOLD_BYTES`, which the measurement sets to `usize::MAX`:
**no size class won, so `auto` stays on the CPU** and the default build never
even initializes the device (the size check precedes `available()`).
`LIBXML_RS_ACCEL=cuda` forces the GPU for diagnosis and the differential court.

The crossover is not "large files vs small": both single files and batches are
transfer-bound (PCIe 4.0 x16 ≈ 25 GB/s pinned, vs a CPU prepass already at
~22–25 GB/s). A GPU win requires the input to be **already device-resident**, in
which case the kernel is ~9× the CPU prepass (256 MiB: 1.1 ms vs 10.5 ms). That
is a genuine accelerator result, and it is why the path is kept, measured, and
documented rather than deleted — but it does not justify `auto` on any measured
host workload.

## 6. §16.9.6 — correctness

Two independent differentials pin the GPU output:

1. **In-crate** `cuda_index_matches_scalar_on_adversarial_inputs` — an
   independent scalar reference vs the GPU over: every block/offset alignment,
   terminators planted at block boundaries ±, huge clean runs, comments, CDATA,
   DOCTYPE/internal subsets, entity references, long attributes, valid and
   malformed Unicode, malformed XML, 300 deterministic random buffers across
   the full byte range and block-boundary lengths, empty and single-byte. Plus
   `cuda_batch_matches_per_document`.
2. **DSO court** `courts/suites/phase16/cuda-run.sh` +
   `cuda-differential.sh` — 182 documents (boundary corpus + explicit
   DOCTYPE/entity/Unicode/malformed constructs + phase-14 fixtures) parsed by a
   `--features cuda` candidate under `LIBXML_RS_ACCEL=cpu|cuda` × thread counts
   × forced prepass. **G1 ×7 and G2 ×2 all PASS: stdout and stderr are
   byte-identical (164 491 rows, 661 diagnostics).** A precondition fails the
   court if the DSO did not actually initialize CUDA, so the comparison cannot
   be vacuous.

Fail-closed: every entry point returns `Option`; any driver/init/allocation/
launch error falls back to the CPU scanner, and a parse never fails because
CUDA failed.

### 6.1 Bug found and fixed by this court

The first court run failed on `cuda_t*` stderr with
`failed to load "…bogus.xml": ` (empty errno text) vs
`…: No such file or directory`. Root cause: **two different I/O-warning
builders disagreed when `errno == 0`** —
`exports_parser.rs` (the entity-loader fallback) used an *empty* string while
`io_load_failure_message` used the `XML_IO_ENOENT` table text — and `errno` was
read lazily at message time, so the optional accelerator's driver calls
clobbered it. Fix: capture the errno of the failed `open(2)` immediately
(`io::note_load_errno`) and route the duplicated site through the single
`io_load_failure_message` builder. The diagnostic is now deterministic across
CPU/GPU configurations (and the two sites agree byte-for-byte).

## 7. Bugs found and fixed while running the phase gates

### 7.1 CUDA exposed a nondeterministic I/O diagnostic

The first court run failed on `cuda_t*` stderr with
`failed to load "…bogus.xml": ` (empty errno text) vs
`…: No such file or directory`. Root cause: **two different I/O-warning
builders disagreed when `errno == 0`** — `exports_parser.rs` (the entity-loader
fallback) used an *empty* string while `io_load_failure_message` used the
`XML_IO_ENOENT` table text — and `errno` was read lazily at message time, so the
optional accelerator's driver calls clobbered it. Fix: capture the errno of the
failed `open(2)` immediately (`io::note_load_errno`) and route the duplicated
site through the single `io_load_failure_message` builder. The diagnostic is now
deterministic across CPU/GPU configurations (and the two sites agree
byte-for-byte).

### 7.2 `xpath` fuzzer: `name()` dereferenced a NULL context node

`cargo fuzz run xpath` found `&name()` aborting in
`xpath::functions::fn_name` (null-pointer dereference). `get_first_node`
returned `Some(ctx.context_node)` even when the context node was NULL — which
is the normal state for a context created only to register functions (the
`exslt::xpath_ctxt_register_module` path). Fix: a NULL node is not a node;
`get_first_node` now filters NULL, so `name()`, `local-name()`,
`namespace-uri()` return the empty string instead of dereferencing. Regression
test `test_name_functions_with_null_context_node`; `xpath` fuzz clean (701 626
runs / 61 s).

## 8. Gates

| Gate | Result |
|---|---|
| `cargo test --lib` (feature off) | 1347 passed / 0 failed |
| `cargo test --lib --features cuda` | +3 passed (incl. adversarial differential), 1 ignored bench |
| CUDA DSO differential (`cuda-run.sh`) | precondition PASS; G1 ×7, G2 ×2 byte-identical |
| `cargo fmt --check` | clean |
| lxml full suite | Ran 2007 tests — OK (0 failures) |
| PHP six-gate | 1250 passed / 0 failed |
| §16.8 courts (re-run after the errno fix) | unchanged, 15/15 PASS |
| §16.7 court (re-run) | G1 PASS; G2 trees byte-identical |
| fuzz html / xpath | clean (xpath crash fixed, §7.2) |
| fuzz parse | pre-existing DTD entity leak, tracked in `16-8-rayon.md` §10.2 |
