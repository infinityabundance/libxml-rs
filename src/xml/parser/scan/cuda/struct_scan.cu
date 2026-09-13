// §16.9 — Stage-1 structural classifier (device side).
//
// Massively data-parallel, exactly the §16.9.2 Stage-1 task list: for every
// `COARSE`-byte block of the input, classify each byte into the four content
// classes the CPU tokenizer uses and compact the result into
//
//   * `first4[block*4 + c]` — the block-local offset of the FIRST terminator of
//     class c (text=0, comment=1, CDATA=2, PI=3), or 0xFFFF when the block is
//     clean for c. This is bit-for-bit the CPU `StructIndex` table
//     (`scan::parallel::summarize_block`), so CPU Stage-2 (`run_len_indexed`) is
//     unchanged.
//   * `stats4[block*4 + k]` — per-block counts: k=0 text terminators, k=1 bytes
//     >= 0x80 (ASCII/non-ASCII, §16.8.3 Level B), k=2 CR/LF line breaks, k=3
//     quote bytes (`"`/`'`, a Stage-2 attribute hint).
//
// Blocks are laid out at `block * COARSE` in `data`; `ends[block]` is the real
// end offset of that block (so a trailing partial block is clamped without
// padding the upload). This single form serves BOTH the single-document case
// (`ends[b] = min(b*COARSE+COARSE, len)`) and the §16.9.3 batch case (each
// document placed at a COARSE-aligned device offset, so no block straddles two
// documents; each source stays semantically independent).
//
// One warp processes one block. Each lane strides it by 32 bytes and reduces
// the per-lane minima/counts with warp intrinsics (SM 8.0+), so the kernel is
// memory-bandwidth-bound even at 4 KiB blocks.
//
// The class predicates are the SAME `TERM_TABLE` bits the CPU scanner uses; a
// single wrong bit is a parser correctness defect (§16.9.6), so this kernel is
// differentially pinned against the scalar scanner.
//
// Build (see PROVENANCE.md): nvcc -ptx -arch=compute_80 -O3 -o struct_scan.ptx
// struct_scan.cu

#define COARSE 4096

// Bit c set iff the byte terminates class c, matching the CPU TERM_TABLE.
__device__ __forceinline__ unsigned int term_mask(unsigned char b) {
    bool printable = (b >= 0x20u && b <= 0x7Eu);
    unsigned int m = 0u;
    // text: printable ASCII except '<', '&', ']'
    if (!(printable && b != '<' && b != '&' && b != ']')) m |= 1u;
    // comment: printable ASCII except '-'
    if (!(printable && b != '-')) m |= 2u;
    // CDATA: printable ASCII except ']'
    if (!(printable && b != ']')) m |= 4u;
    // PI: printable ASCII except '?'
    if (!(printable && b != '?')) m |= 8u;
    return m;
}

#define NONE 0xFFFFFFFFu

extern "C" __global__ void struct_scan(
    const unsigned char* __restrict__ data,
    const unsigned int* __restrict__ ends,
    unsigned int nblocks,
    unsigned short* __restrict__ first4,   // nblocks * 4
    unsigned int* __restrict__ stats4)      // nblocks * 4
{
    unsigned int warps_per_cta = blockDim.x >> 5;
    unsigned int block = blockIdx.x * warps_per_cta + (threadIdx.x >> 5);
    unsigned int lane = threadIdx.x & 31u;
    unsigned int w = block * 4u;
    if (block >= nblocks) return;

    unsigned int base = block * COARSE;
    unsigned int end = ends[block];

    unsigned int f0 = NONE, f1 = NONE, f2 = NONE, f3 = NONE;
    unsigned int csp = 0u, cna = 0u, clb = 0u, cq = 0u;

    if (end > base) {
        for (unsigned int i = base + lane; i < end; i += 32u) {
            unsigned char b = data[i];
            unsigned int m = term_mask(b);
            if (m != 0u) {
                unsigned int off = i - base;
                if ((m & 1u) && f0 == NONE) f0 = off;
                if ((m & 2u) && f1 == NONE) f1 = off;
                if ((m & 4u) && f2 == NONE) f2 = off;
                if ((m & 8u) && f3 == NONE) f3 = off;
                // stats[0] is the TEXT-terminator count (matches the CPU
                // BlockSummary.specials), not "any class".
                if (m & 1u) csp++;
            }
            if (b >= 0x80u) cna++;
            if (b == '\n' || b == '\r') clb++;
            if (b == '"' || b == '\'') cq++;
        }
    }

    f0 = __reduce_min_sync(NONE, f0);
    f1 = __reduce_min_sync(NONE, f1);
    f2 = __reduce_min_sync(NONE, f2);
    f3 = __reduce_min_sync(NONE, f3);
    csp = __reduce_add_sync(NONE, csp);
    cna = __reduce_add_sync(NONE, cna);
    clb = __reduce_add_sync(NONE, clb);
    cq = __reduce_add_sync(NONE, cq);

    if (lane == 0u) {
        first4[w + 0u] = (unsigned short)(f0 == NONE ? 0xFFFFu : f0);
        first4[w + 1u] = (unsigned short)(f1 == NONE ? 0xFFFFu : f1);
        first4[w + 2u] = (unsigned short)(f2 == NONE ? 0xFFFFu : f2);
        first4[w + 3u] = (unsigned short)(f3 == NONE ? 0xFFFFu : f3);
        stats4[w + 0u] = csp;
        stats4[w + 1u] = cna;
        stats4[w + 2u] = clb;
        stats4[w + 3u] = cq;
    }
}
