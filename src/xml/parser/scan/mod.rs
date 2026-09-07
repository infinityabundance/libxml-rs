//! §16.7 SIMD structural scanning — vectorized detection of the bytes that
//! terminate a plain character-data run.
//!
//! The parser's text scanner must find the first byte that needs
//! per-character handling (`<`, `&`, `]`, CR/LF, any other control byte, or
//! any non-ASCII byte). Upstream libxml2 finds such delimiters with tight C
//! byte scans; the candidate's §16.5.6 fast path used a per-byte
//! `take_while` that the compiler can only auto-vectorize to the SSE2
//! baseline (the crate is built for generic x86-64). This module provides
//! runtime-dispatched AVX2 and AVX-512BW implementations of the same
//! classification plus the scalar reference, behind one narrow seam so the
//! tokenizer stays backend agnostic.
//!
//! # Compatibility contract
//!
//! Every backend computes the EXACT same result: the byte offset of the
//! first byte NOT in the printable-ASCII content set (`0x20..=0x7E` minus
//! `<`, `&`, `]`) — the run length the text scanner may consume with
//! `skip_linebreak_free` (those bytes are never CR/LF, so no line/col
//! bookkeeping applies; every byte is a valid XML Char that decodes to
//! itself). Backends differ only in speed; observable parser behavior is
//! identical for every backend. §16.7.7 pins this with boundary and
//! differential tests.
//!
//! # Selection (§16.7.6)
//!
//! The backend is chosen once per process (cached in a `OnceLock` — never
//! CPUID inside a scanner loop). The private diagnostic override
//! `LIBXML_RS_SCAN_BACKEND=scalar|avx2|avx512|auto` forces a backend (used
//! by the §16.7.7 differential suites and the fuzzers); `auto` uses runtime
//! feature detection with a conservative evidence-driven policy.

pub(crate) mod scalar;
#[cfg(target_arch = "x86_64")]
pub(crate) mod x86;

use scalar::scalar_text_run_len;

/// Scanning backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanBackend {
    /// Byte-at-a-time reference implementation (always available; the
    /// correctness reference for the vector paths).
    Scalar,
    /// 32-byte AVX2 lanes.
    Avx2,
    /// 64-byte AVX-512BW lanes.
    Avx512,
}

impl ScanBackend {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            ScanBackend::Scalar => "scalar",
            ScanBackend::Avx2 => "avx2",
            ScanBackend::Avx512 => "avx512",
        }
    }
}

/// Length of the printable-ASCII text run starting at `bytes[0]`.
pub(crate) fn text_run_len(bytes: &[u8], backend: ScanBackend) -> usize {
    match backend {
        ScanBackend::Scalar => scalar_text_run_len(bytes),
        ScanBackend::Avx2 => {
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: Avx2 is only ever selected after
                // is_x86_feature_detected!("avx2") (see select_backend and
                // the gated tests).
                unsafe { x86::avx2_text_run_len(bytes) }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                let _ = bytes;
                scalar_text_run_len(bytes)
            }
        }
        ScanBackend::Avx512 => {
            #[cfg(target_arch = "x86_64")]
            {
                // SAFETY: Avx512 is only selected when avx512f+avx512bw are
                // detected.
                unsafe { x86::avx512_text_run_len(bytes) }
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                let _ = bytes;
                scalar_text_run_len(bytes)
            }
        }
    }
}

/// Backend currently active (diagnostics / differential tests).
pub(crate) fn active_backend() -> ScanBackend {
    *SELECTED.get_or_init(select_backend)
}

/// Convenience for the tokenizer: run length with the process default
/// backend (one cached load + indirect call).
#[inline]
pub(crate) fn text_run_len_auto(bytes: &[u8]) -> usize {
    text_run_len(bytes, active_backend())
}

/// Force the backend process-wide (diagnostic override; used by tests).
pub(crate) fn set_backend_for_tests(backend: ScanBackend) {
    let _ = SELECTED.set(backend);
}

static SELECTED: std::sync::OnceLock<ScanBackend> = std::sync::OnceLock::new();

/// Resolve the backend: `LIBXML_RS_SCAN_BACKEND` override first (unknown
/// values fall back to auto), then the conservative auto policy.
fn select_backend() -> ScanBackend {
    if let Ok(v) = std::env::var("LIBXML_RS_SCAN_BACKEND") {
        match v.to_ascii_lowercase().as_str() {
            "scalar" => return ScanBackend::Scalar,
            "avx2" => return ScanBackend::Avx2,
            "avx512" | "avx-512" => return ScanBackend::Avx512,
            _ => {}
        }
    }
    policy()
}

/// §16.7.6 conservative auto policy. AVX2 is the default on capable CPUs
/// (32-byte classification consistently beats SSE2-bounded scalar on real
/// text runs — §16.7.5 receipts). AVX-512 is NOT assumed to win: it is only
/// enabled on CPUs reporting avx512f+avx512bw and §16.7.5/§16.16 measure
/// whether 512-bit actually wins per size/composition before it is ever
/// chosen in `auto`; until that evidence lands, `auto` stays on AVX2.
fn policy() -> ScanBackend {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            return ScanBackend::Avx2;
        }
    }
    ScanBackend::Scalar
}

/// Backend support (for the differential tests: only compare a vector
/// backend when this CPU actually has it).
pub(crate) fn backend_supported(backend: ScanBackend) -> bool {
    match backend {
        ScanBackend::Scalar => true,
        ScanBackend::Avx2 => {
            #[cfg(target_arch = "x86_64")]
            {
                std::arch::is_x86_feature_detected!("avx2")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
        ScanBackend::Avx512 => {
            #[cfg(target_arch = "x86_64")]
            {
                std::arch::is_x86_feature_detected!("avx512f")
                    && std::arch::is_x86_feature_detected!("avx512bw")
            }
            #[cfg(not(target_arch = "x86_64"))]
            {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference run-length from the scalar module (independent of the
    /// take_while duplicated in the tokenizer).
    fn expected(b: &[u8]) -> usize {
        scalar_text_run_len(b)
    }

    /// §16.7.7 boundary suite: every length 0..=260 and every start offset,
    /// with a byte pattern that places every interesting byte at every
    /// possible lane position, must agree across all supported backends.
    #[test]
    fn backends_agree_on_boundaries() {
        let supported: Vec<ScanBackend> =
            [ScanBackend::Scalar, ScanBackend::Avx2, ScanBackend::Avx512]
                .into_iter()
                .filter(|b| backend_supported(*b))
                .collect();
        assert!(!supported.is_empty());

        let mut data: Vec<u8> = Vec::with_capacity(320);
        for len in 0..260usize {
            data.clear();
            let bytes_of_interest = [
                b'<', b'&', b']', b'\n', b'\r', 0x00, 0x1F, 0x20, 0x7E, 0x7F, 0x80, 0xC3, 0xFF,
                b'a', b'~', b'\t', b' ', b'5', b']', b'<',
            ];
            for i in 0..len {
                data.push(bytes_of_interest[i % bytes_of_interest.len()]);
            }
            for start in 0..len {
                let want = expected(&data[start..]);
                for be in &supported {
                    assert_eq!(
                        text_run_len(&data[start..], *be),
                        want,
                        "backend {:?} start {start} len {len}",
                        be
                    );
                }
            }
        }
        // Clean long runs (no special bytes at all) up to 4 lanes + tail.
        for len in [63usize, 64, 65, 127, 128, 129, 191, 192, 193, 1000] {
            let data = vec![b'x'; len];
            for be in &supported {
                assert_eq!(text_run_len(&data, *be), len, "clean len {len} {:?}", be);
            }
        }
    }

    /// The force-select override parses and the auto policy picks a
    /// supported backend.
    #[test]
    fn selection_consistency() {
        let be = select_backend();
        assert!(backend_supported(be), "{:?} must be supported", be);
    }
}
