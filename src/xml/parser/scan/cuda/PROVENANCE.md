# §16.9 CUDA device-code provenance

`struct_scan.ptx` is the checked-in device image embedded by
`src/xml/parser/scan/cuda/mod.rs` (`include_str!`). It is forward-compatible
PTX, JIT-compiled by the installed NVIDIA driver at `cuModuleLoadData` time, so
a consumer does **not** need `nvcc` or a toolkit — only a driver new enough to
JIT the recorded `.version`/`.target` (and a GPU of `sm_80` or newer).

## Generation

```
cd src/xml/parser/scan/cuda
/opt/cuda/bin/nvcc -ptx -arch=compute_80 -O3 -o struct_scan.ptx struct_scan.cu
```

| Field | Value |
|---|---|
| Source | `struct_scan.cu` (same directory) |
| Compiler | `nvcc` release 13.3, V13.3.73 (build `cuda_13.3.r13.3/compiler.38244171_0`) |
| Flags | `-ptx -arch=compute_80 -O3` |
| PTX ISA | `.version 9.3` |
| Target | `.target sm_80` (JIT for `sm_80`+) |
| Output SHA-256 | see `struct_scan.ptx.sha256` |
| Host used to generate | AMD Ryzen 7 9800X3D + NVIDIA RTX 4080 SUPER (SM 8.9), driver 615.71.09 |

Regenerate with `tools/cuda/regen-ptx.sh` (records the `nvcc` version and
refreshes the SHA-256). The `.ptx` is the artefact of record; the `.cu` is the
reviewable source.

## Why PTX, not a cubin

A cubin is tied to one compute capability; PTX is JIT'd for whatever GPU is
present (and for the same GPU across driver upgrades). If JIT fails (too-old
driver, pre-`sm_80` GPU, no device), `cuda::state()` returns `None` and every
parse falls back to the CPU scanner — an XML parse never fails because CUDA
failed (§16.9.6).
