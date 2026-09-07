//! Scalar reference implementation of the §16.7 text-run classification.
//!
//! This is the correctness reference every vector backend must match
//! byte-for-byte, and the fallback on CPUs without AVX2/AVX-512.

/// Length of the leading printable-ASCII content run: bytes in
/// `0x20..=0x7E` excluding `<`, `&`, `]` (§16.5.6 set — never CR/LF, so no
/// line/col bookkeeping applies; every byte is a valid XML Char that
/// decodes to itself).
pub(crate) fn scalar_text_run_len(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .take_while(|&&b| (0x20..=0x7E).contains(&b) && b != b'<' && b != b'&' && b != b']')
        .count()
}
