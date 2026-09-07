//! x86-64 SIMD implementations of the §16.7 text-run classification
//! (AVX2 and AVX-512BW).
//!
//! # Safety model
//!
//! Both functions are `#[target_feature]`-gated and MUST only be invoked
//! after the corresponding `is_x86_feature_detected!` check (see the
//! dispatcher in `super::text_run_len` / `policy`). They read up to
//! `bytes.len()` bytes; the final partial vector is loaded with
//! `_mm256_maskload_epi32`/`_mm512_maskload_epi32`-style masking or an
//! exact bounded tail loop, so they never read past the slice.
//!
//! Both return the offset of the first byte NOT in
//! `0x20..=0x7E` minus `<`/`&`/`]` — identical to
//! [`scalar_text_run_len`](super::scalar::scalar_text_run_len).

#![cfg(target_arch = "x86_64")]

use core::arch::x86_64::*;

/// 32-byte AVX2 classification of a text-run prefix.
///
/// # Safety
///
/// - The caller guarantees AVX2 support (`is_x86_feature_detected!("avx2")`).
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn avx2_text_run_len(bytes: &[u8]) -> usize {
    let mut off = 0usize;
    let len = bytes.len();
    // 32-byte masked loads keep the tail in-bounds (masked-off lanes read
    // nothing), so a full 32-byte chunk can be examined even at the end.
    while off < len {
        let chunk = if len - off >= 32 {
            _mm256_loadu_si256(bytes.as_ptr().add(off) as *const __m256i)
        } else {
            // Masked load: high lanes zeroed and excluded from the result
            // mask below (their lt/hi/eq tests evaluate to 0 for zero bytes
            // since 0 is < 0x20 -> would falsely stop; the tail handling
            // below clamps to len - off instead).
            return off + scalar_tail(&bytes[off..]);
        };
        let run = classify32(chunk);
        if run != 32 {
            return off + run;
        }
        off += 32;
    }
    len
}

/// Classify 32 bytes: count of leading content bytes (returns 32 when all
/// are content bytes).
#[target_feature(enable = "avx2")]
unsafe fn classify32(chunk: __m256i) -> usize {
    // lt = b < 0x20 ; gt = b > 0x7E  (unsigned comparisons via min/max)
    let lt = _mm256_cmpeq_epi8(_mm256_min_epu8(chunk, _mm256_set1_epi8(0x1F)), chunk);
    let gt = _mm256_cmpeq_epi8(_mm256_max_epu8(chunk, _mm256_set1_epi8(0x7F)), chunk);
    // eq to the three structural bytes
    let eq_lt = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'<' as i8));
    let eq_amp = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b'&' as i8));
    let eq_rb = _mm256_cmpeq_epi8(chunk, _mm256_set1_epi8(b']' as i8));
    // stop = lt | gt | eq_lt | eq_amp | eq_rb
    let stop = _mm256_or_si256(
        _mm256_or_si256(_mm256_or_si256(lt, gt), _mm256_or_si256(eq_lt, eq_amp)),
        eq_rb,
    );
    let mask = _mm256_movemask_epi8(stop) as u32;
    let first = mask.trailing_zeros() as usize;
    if first < 32 {
        first
    } else {
        32
    }
}

/// 64-byte AVX-512BW classification of a text-run prefix.
///
/// # Safety
///
/// - The caller guarantees avx512f + avx512bw support.
#[target_feature(enable = "avx512f,avx512bw")]
pub(crate) unsafe fn avx512_text_run_len(bytes: &[u8]) -> usize {
    let mut off = 0usize;
    let len = bytes.len();
    while off < len {
        if len - off >= 64 {
            let chunk = _mm512_loadu_si512(bytes.as_ptr().add(off) as *const __m512i);
            let run = classify64(chunk);
            if run != 64 {
                return off + run;
            }
            off += 64;
        } else {
            return off + scalar_tail(&bytes[off..]);
        }
    }
    len
}

/// Classify 64 bytes: count of leading content bytes (returns 64 when all
/// are content bytes).
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn classify64(chunk: __m512i) -> usize {
    // unsigned byte compares via avx512bw compare intrinsics
    let lt = _mm512_cmplt_epu8_mask(chunk, _mm512_set1_epi8(0x20));
    let gt = _mm512_cmpgt_epu8_mask(chunk, _mm512_set1_epi8(0x7E));
    let eq_lt = _mm512_cmpeq_epi8_mask(chunk, _mm512_set1_epi8(b'<' as i8));
    let eq_amp = _mm512_cmpeq_epi8_mask(chunk, _mm512_set1_epi8(b'&' as i8));
    let eq_rb = _mm512_cmpeq_epi8_mask(chunk, _mm512_set1_epi8(b']' as i8));
    let stop = lt | gt | eq_lt | eq_amp | eq_rb;
    let first = stop.trailing_zeros() as usize;
    if first < 64 {
        first
    } else {
        64
    }
}

/// Exact bounded tail scan (never reads past `bytes.len()`).
fn scalar_tail(bytes: &[u8]) -> usize {
    super::scalar::scalar_text_run_len(bytes)
}
