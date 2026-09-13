//! §16.8 Level A/B — parallel structural scanning of independent byte blocks.
//!
//! The parser's hot scans are the *content-run* classifiers: find the first
//! byte that ends a run of bytes the tokenizer can consume verbatim. Three
//! classes matter:
//!
//! - text content ([`super::text_run_len`]): stop at `<`, `&`, `]`, CR/LF, any
//!   control, or non-ASCII;
//! - comment content: stop at `-` (the `-->`/`--` gate), CR/LF, or non-ASCII;
//! - CDATA content: stop at `]` (the `]]>` gate), CR/LF, or non-ASCII;
//! - PI content: stop at `?` (the `?>` gate), CR/LF, or non-ASCII.
//!
//! The §16.7 SIMD backends made the text class fast single-threaded. The
//! comment/CDATA/PI classes were scanned one decoded character at a time, which
//! is where a large comment or CDATA section spends nearly all of its parse
//! time. This module classifies those runs across the §16.8.1 private pool.
//!
//! # Why this shape (prefix reconciliation, §16.8.2)
//!
//! Parallel XML scanning is not "split the bytes and parse each part". The
//! safest parallel operation is to classify independent blocks and *then*
//! reconcile their starting lexical state. For a content-run class the
//! reconciliation is exact and cheap: the class is **context-free** — a byte is
//! a run terminator regardless of what precedes it — so the starting state of a
//! block is simply "the run was still open at the block boundary", which the
//! sequential prefix establishes. The first block (in document order) that
//! contains a terminator wins; blocks after it are never inspected for a
//! *first* match, so no later block can change an earlier answer.
//!
//! The context-sensitive lexical reconciliation that §16.8.2 describes (quote /
//! comment / CDATA / PI *transitions*) is required only for a scanner that
//! *interprets* `<`/`&`/`-`/`]`/`?` per region; the tokenizer's run classifier
//! does not — it is invoked in exactly one lexical state at a time and this
//! consumer never re-interprets a skipped byte. [`structural_summary`] still
//! computes the §16.8.2 block masks (terminator, non-ASCII, line-break) with a
//! real cross-block prefix accumulation, and the differential court checks them
//! against a scalar reference.
//!
//! # Cost shape
//!
//! A sequential prefix of one [`BLOCK`] is always scanned first. Typical markup
//! has short runs that end inside that prefix, so the pool is never dispatched
//! and there is **no overhead** versus the sequential scanner. Only when the
//! prefix is entirely clean (a long run) does the pool scan the remainder in
//! ordered waves, so at most one wave of work past the true terminator is ever
//! wasted. Below the `auto` crossover the exact sequential classifier runs.

use rayon::prelude::*;

use super::pool::{self, Config, BLOCK};
use super::{active_backend, text_run_len};

/// Blocks per parallel wave, as a multiple of the worker count.
const WAVE_MULT: usize = 4;

/// The content classes the tokenizer consumes verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanClass {
    /// Comment content: printable ASCII except `-`.
    Comment,
    /// CDATA content: printable ASCII except `]`.
    Cdata,
    /// Processing-instruction content: printable ASCII except `?`.
    Pi,
}

impl CleanClass {
    /// Whether `b` may be consumed verbatim without a per-character decision.
    #[inline]
    pub(crate) fn is_clean(self, b: u8) -> bool {
        let printable = (0x20..=0x7E).contains(&b);
        match self {
            CleanClass::Comment => printable && b != b'-',
            CleanClass::Cdata => printable && b != b']',
            CleanClass::Pi => printable && b != b'?',
        }
    }
}

/// Generic ordered-wave first-match over independent blocks.
///
/// Returns `(index, used_pool)`. The `used_pool` flag lets the differential
/// court prove the parallel decomposition actually ran (and that the sequential
/// prefix short-circuits the common short-run case).
fn first_match_with<F>(bytes: &[u8], cfg: &Config, find: &F) -> (Option<usize>, bool)
where
    F: Fn(&[u8]) -> Option<usize> + Sync,
{
    let n = bytes.len();
    if n == 0 {
        return (None, false);
    }
    // Below the crossover (or with parallelism off / a single CPU) stay exactly
    // on the sequential path.
    if !cfg.per_call_engages(n) {
        return (find(bytes), false);
    }

    // Sequential prefix: covers every run shorter than one block with no pool
    // dispatch at all (the common markup case), and establishes the reconciled
    // starting state ("run still open") for the parallel tail.
    let prefix = n.min(BLOCK);
    if let Some(i) = find(&bytes[..prefix]) {
        return (Some(i), false);
    }

    // Parallel tail, ordered waves of indexed blocks. `par_chunks` gives
    // uniform, indexed blocks (§16.8.6); each worker writes its own
    // `Option<usize>` into the collected vector (no lock, no `par_bridge`).
    let wave_blocks = cfg.threads.max(1).saturating_mul(WAVE_MULT).max(1);
    let mut off = prefix;
    while off < n {
        let end = off.saturating_add(wave_blocks.saturating_mul(BLOCK)).min(n);
        let span = &bytes[off..end];
        let hits: Vec<Option<usize>> =
            pool::install(|| span.par_chunks(BLOCK).map(|block| find(block)).collect());
        // First hit in index order is the first terminator in the document.
        for (i, hit) in hits.into_iter().enumerate() {
            if let Some(local) = hit {
                return (Some(off + i * BLOCK + local), true);
            }
        }
        off = end;
    }
    (None, true)
}

/// Index of the first text-content run terminator, or `None` when every byte is
/// content. Equivalent to `text_run_len(bytes) < bytes.len()`.
pub(crate) fn first_special(bytes: &[u8]) -> Option<usize> {
    first_special_traced(bytes, pool::config()).0
}

/// Like [`first_special`], also reporting whether the private pool was engaged.
pub(crate) fn first_special_traced(bytes: &[u8], cfg: &Config) -> (Option<usize>, bool) {
    let backend = active_backend();
    let find = |b: &[u8]| {
        let run = text_run_len(b, backend);
        (run < b.len()).then_some(run)
    };
    first_match_with(bytes, cfg, &find)
}

/// Direct sequential text-run length (no config/`engage` re-check). Used as
/// the local fallback when the structural index cannot answer an in-block
/// lookup (dense markup).
#[inline]
pub(crate) fn text_run_len_local(bytes: &[u8]) -> usize {
    text_run_len(bytes, active_backend())
}

/// Direct sequential class-run length (no config/`engage` re-check).
#[inline]
pub(crate) fn clean_run_len_local(bytes: &[u8], class: CleanClass) -> usize {
    bytes
        .iter()
        .position(|&b| !class.is_clean(b))
        .unwrap_or(bytes.len())
}

/// Class-run length for the comment / CDATA / PI fast paths, honouring the
/// resolved [`Config`]: `PARALLEL=off` and `auto` run the exact sequential
/// classifier, `PARALLEL=on` uses the per-call ordered-wave pool scan.
///
/// Under `auto` the *tokenizer* is responsible for escalating a run that
/// reaches the §16.8.7 crossover to the whole-input [`StructIndex`] (see
/// [`probe_class_run`] and `XmlTokenizer::class_content_run`): a per-call pool
/// dispatch is only amortised when it replaces a long *per-character* scan, and
/// the [`StructIndex`] replaces those scans for every run after the first.
#[inline]
pub(crate) fn clean_run_len_class(bytes: &[u8], class: CleanClass) -> usize {
    let find = |b: &[u8]| b.iter().position(|&x| !class.is_clean(x));
    first_match_with(bytes, pool::config(), &find)
        .0
        .unwrap_or(bytes.len())
}

/// Outcome of a bounded class-run probe ([`probe_class_run`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassProbe {
    /// The run's exact length, resolved within the probe budget.
    Exact(usize),
    /// The first `budget` bytes are all clean: the run is at least that long,
    /// so the caller escalates to the whole-input [`StructIndex`].
    Long,
}

/// Probe the length of the `class` content run at `bytes[0]` without scanning
/// the whole run: an exact answer is returned when the run terminates within
/// `budget` bytes, otherwise [`ClassProbe::Long`].
///
/// §16.8.7: the tokenizer uses this with `budget = threshold` so a run shorter
/// than the measured crossover is resolved sequentially (no pool dispatch, no
/// prepass) while only a run that can amortise the whole-input index triggers
/// it. The scan is exact: the returned `Exact(n)` is the first terminator
/// offset (or `bytes.len()` when the run reaches the end of the input).
#[inline]
pub(crate) fn probe_class_run(bytes: &[u8], class: CleanClass, budget: usize) -> ClassProbe {
    let n = bytes.len().min(budget);
    match bytes[..n].iter().position(|&b| !class.is_clean(b)) {
        Some(i) => ClassProbe::Exact(i),
        None if n == bytes.len() => ClassProbe::Exact(bytes.len()),
        None => ClassProbe::Long,
    }
}

/// Run length for the tokenizer's text fast path: the count of leading content
/// bytes. This is the parallel drop-in for [`super::text_run_len_auto`].
#[inline]
pub(crate) fn content_run_len(bytes: &[u8]) -> usize {
    // A slice smaller than one block can never engage the pool (the threshold
    // is at least one block), so call the classifier directly to avoid the
    // config/`engage` overhead on the common short-run call.
    if bytes.len() < BLOCK {
        return text_run_len(bytes, active_backend());
    }
    first_special(bytes).unwrap_or(bytes.len())
}

/// Run length of bytes consumable verbatim in `class` (comment / CDATA / PI
/// content), using the private pool for long runs.
///
/// Exactness: the returned prefix contains only bytes the sequential per-char
/// loop in `tokenizer.rs` would have consumed one at a time with no line/column
/// or encoding bookkeeping (the class excludes CR/LF and every byte >= 0x80),
/// so a bulk skip is indistinguishable from the character loop.
#[inline]
pub(crate) fn clean_run_len(bytes: &[u8], class: CleanClass) -> usize {
    if bytes.len() < BLOCK {
        return bytes
            .iter()
            .position(|&b| !class.is_clean(b))
            .unwrap_or(bytes.len());
    }
    let find = |b: &[u8]| b.iter().position(|&x| !class.is_clean(x));
    first_match_with(bytes, pool::config(), &find)
        .0
        .unwrap_or(bytes.len())
}

/// Index of the first `needle` byte, or `None`. Same decomposition.
pub(crate) fn first_byte(bytes: &[u8], needle: u8) -> Option<usize> {
    first_byte_traced(bytes, needle, pool::config()).0
}

/// Like [`first_byte`], also reporting whether the private pool was engaged.
pub(crate) fn first_byte_traced(bytes: &[u8], needle: u8, cfg: &Config) -> (Option<usize>, bool) {
    let find = |b: &[u8]| b.iter().position(|&x| x == needle);
    first_match_with(bytes, cfg, &find)
}

// ═════════════════════════════════════════════════════════════════════════════
// §16.8.2 structural summary (block masks + cross-block reconciliation)
// ═════════════════════════════════════════════════════════════════════════════

/// Per-block structural masks (§16.8.2): the block-local first text-content
/// terminator, and the counts of terminators, non-ASCII bytes and line breaks.
/// Counts are context-free and therefore reconcile across blocks by simple
/// prefix accumulation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BlockSummary {
    /// Block-local offset of the first text-content terminator, if any.
    pub first_special: Option<u32>,
    /// Number of text-content terminator bytes in the block.
    pub specials: u32,
    /// Number of bytes >= 0x80 in the block.
    pub non_ascii: u32,
    /// Number of CR/LF bytes in the block.
    pub linebreaks: u32,
}

impl BlockSummary {
    fn of(block: &[u8]) -> Self {
        let mut s = BlockSummary::default();
        for (i, &b) in block.iter().enumerate() {
            let special = !(0x20..=0x7E).contains(&b) || b == b'<' || b == b'&' || b == b']';
            if special {
                if s.first_special.is_none() {
                    s.first_special = Some(i as u32);
                }
                s.specials += 1;
            }
            if b >= 0x80 {
                s.non_ascii += 1;
            }
            if b == b'\n' || b == b'\r' {
                s.linebreaks += 1;
            }
        }
        s
    }
}

/// The reconciled whole-input structural model (§16.8.2). `blocks` is in
/// document order; `special_at`/`linebreak_at` are the prefix-accumulated
/// totals entering each block (a compact prefix reconciliation).
#[derive(Debug, Clone)]
pub(crate) struct StructuralSummary {
    pub blocks: Vec<BlockSummary>,
    /// `special_at[i]` = total specials in blocks `0..i`.
    pub special_at: Vec<u64>,
    /// `linebreak_at[i]` = total line breaks in blocks `0..i`.
    pub linebreak_at: Vec<u64>,
    pub len: usize,
    pub threads: usize,
}

impl StructuralSummary {
    pub fn total_specials(&self) -> u64 {
        self.special_at.last().copied().unwrap_or(0)
            + self.blocks.last().map_or(0, |b| b.specials as u64)
    }
    pub fn total_non_ascii(&self) -> u64 {
        self.blocks.iter().map(|b| b.non_ascii as u64).sum()
    }
    pub fn total_linebreaks(&self) -> u64 {
        self.linebreak_at.last().copied().unwrap_or(0)
            + self.blocks.last().map_or(0, |b| b.linebreaks as u64)
    }
}

/// Build the §16.8.2 block summaries for `bytes`, in parallel when eligible.
pub(crate) fn structural_summary(bytes: &[u8]) -> StructuralSummary {
    structural_summary_with(bytes, pool::config())
}

fn structural_summary_with(bytes: &[u8], cfg: &Config) -> StructuralSummary {
    let parallel = cfg.engages(bytes.len());
    let blocks: Vec<BlockSummary> = if parallel {
        pool::install(|| bytes.par_chunks(BLOCK).map(BlockSummary::of).collect())
    } else {
        bytes.chunks(BLOCK).map(BlockSummary::of).collect()
    };
    // Compact prefix reconciliation: the context-free counts accumulate in
    // document order. Sequential, O(blocks).
    let mut special_at = Vec::with_capacity(blocks.len() + 1);
    let mut linebreak_at = Vec::with_capacity(blocks.len() + 1);
    let mut s = 0u64;
    let mut l = 0u64;
    for b in &blocks {
        special_at.push(s);
        linebreak_at.push(l);
        s += b.specials as u64;
        l += b.linebreaks as u64;
    }
    StructuralSummary {
        blocks,
        special_at,
        linebreak_at,
        len: bytes.len(),
        threads: if parallel { cfg.threads } else { 1 },
    }
}

/// The four content classes, indexed into [`StructIndex`] block summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunClass {
    Text = 0,
    Comment = 1,
    Cdata = 2,
    Pi = 3,
}

impl RunClass {
    #[inline]
    #[allow(dead_code)] // used by the differential tests
    fn is_terminator(self, b: u8) -> bool {
        match self {
            RunClass::Text => !((0x20..=0x7E).contains(&b) && b != b'<' && b != b'&' && b != b']'),
            RunClass::Comment => !CleanClass::Comment.is_clean(b),
            RunClass::Cdata => !CleanClass::Cdata.is_clean(b),
            RunClass::Pi => !CleanClass::Pi.is_clean(b),
        }
    }
}

/// Coarse block size for the structural index. Each block stores, per class,
/// the block-local offset of the first terminator (or `u16::MAX`). 4 KiB keeps
/// the whole index at `len/4096 * 8` bytes (16 MiB of input -> 32 KiB of index)
/// while a lookup never scans more than one clean block before finding a hit.
pub(crate) const COARSE: usize = 4096;

/// §16.8.2 whole-input structural index: one parallel prepass classifies every
/// coarse block for all four content classes, and the ordered, compact block
/// table is the cross-block reconciliation (context-free classes make the
/// reconciled starting state the identity — see the module docs). The tokenizer
/// then resolves each run in O(1)-amortised time with no per-run pool dispatch.
#[derive(Debug, Clone)]
pub(crate) struct StructIndex {
    /// `blocks[i][class]` = block-local first terminator, or `u16::MAX`.
    blocks: Vec<[u16; 4]>,
    len: usize,
    pub(crate) threads: usize,
}

impl StructIndex {
    /// Build the index for `bytes`, in parallel when eligible. Returns `None`
    /// when the span is below the `auto` crossover (the caller falls back to
    /// the per-call scanner, which is already fast for short inputs).
    pub(crate) fn build(bytes: &[u8]) -> Option<StructIndex> {
        Self::build_with(bytes, pool::config())
    }

    fn build_with(bytes: &[u8], cfg: &Config) -> Option<StructIndex> {
        // §16.9: the optional GPU Stage-1 classifier, when `LIBXML_RS_ACCEL`
        // selects it, produces the identical block table. Fail closed: any
        // device error falls through to the CPU prepass below.
        #[cfg(feature = "cuda")]
        {
            if crate::xml::parser::scan::cuda::select(bytes.len()) {
                if let Some(cs) = crate::xml::parser::scan::cuda::struct_index(bytes) {
                    debug_assert_eq!(cs.blocks.len(), bytes.len().div_ceil(COARSE));
                    return Some(StructIndex {
                        blocks: cs.blocks,
                        len: bytes.len(),
                        threads: 0, // device backend
                    });
                }
            }
        }
        if !cfg.engages(bytes.len()) {
            return None;
        }
        Some(StructIndex {
            blocks: cpu_blocks(bytes, cfg),
            len: bytes.len(),
            threads: cfg.threads,
        })
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Length of the byte run starting at `off` that is clean for `class`, or
    /// `None` when the cursor is already past this coarse block's first
    /// terminator (the caller resolves the remainder locally with the per-call
    /// scanner, which is cheap for the remaining < 4 KiB).
    ///
    /// `data_len` is the total input length (`off + remaining.len()` at the
    /// call site); `Some(r)` never exceeds `data_len - off`.
    #[inline]
    pub(crate) fn run_len(&self, class: RunClass, off: usize, data_len: usize) -> Option<usize> {
        if off >= data_len {
            return Some(0);
        }
        let bi = off / COARSE;
        let within = off % COARSE;
        if let Some(&e) = self.blocks.get(bi) {
            let e = e[class as usize];
            if e != u16::MAX {
                if (e as usize) >= within {
                    return Some(e as usize - within);
                }
                // The block's first terminator lies before the cursor (already
                // consumed); its next terminator is not summarized, so the
                // caller scans the remainder of this block locally.
                return None;
            }
        }
        let mut j = bi + 1;
        while j < self.blocks.len() {
            let e = self.blocks[j][class as usize];
            if e != u16::MAX {
                return Some((j * COARSE + e as usize).min(data_len) - off);
            }
            j += 1;
        }
        Some(data_len - off)
    }
}

/// The CPU structural prepass (§16.8.2): per-coarse-block first terminators,
/// in parallel when the config engages the private pool. Shared by
/// [`StructIndex::build_with`] and the §16.9 CPU-vs-GPU measurement harness.
pub(crate) fn cpu_blocks(bytes: &[u8], cfg: &Config) -> Vec<[u16; 4]> {
    if cfg.threads > 1 {
        pool::install(|| bytes.par_chunks(COARSE).map(summarize_block).collect())
    } else {
        bytes.chunks(COARSE).map(summarize_block).collect()
    }
}

fn summarize_block(block: &[u8]) -> [u16; 4] {
    let mut first = [u16::MAX; 4];
    let mut have = 0u8;
    for (i, &b) in block.iter().enumerate() {
        let m = TERM_TABLE[b as usize] & !have;
        if m != 0 {
            let mut m = m;
            while m != 0 {
                let c = m.trailing_zeros() as usize;
                first[c] = i as u16;
                m &= m - 1;
            }
            have |= TERM_TABLE[b as usize];
            if have == 0xF {
                break;
            }
        }
    }
    first
}

/// For each byte value, a bitmask of the classes it terminates: bit 0 = text,
/// bit 1 = comment, bit 2 = CDATA, bit 3 = PI. A single indexed load per byte
/// replaces four predicates in the structural prepass.
static TERM_TABLE: [u8; 256] = build_term_table();

const fn build_term_table() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut b = 0usize;
    while b < 256 {
        let x = b as u8;
        let printable = x >= 0x20 && x <= 0x7E;
        let mut m = 0u8;
        if !(printable && x != b'<' && x != b'&' && x != b']') {
            m |= 1;
        }
        if !(printable && x != b'-') {
            m |= 2;
        }
        if !(printable && x != b']') {
            m |= 4;
        }
        if !(printable && x != b'?') {
            m |= 8;
        }
        t[b] = m;
        b += 1;
    }
    t
}

/// Resolve the clean-run length for `class` starting at absolute offset
/// `abs_off`, using the prebuilt index where it can answer and a bounded local
/// scan (≤ one coarse block) where it cannot.
///
/// `remaining` is `base[abs_off..]` (the tokenizer's current slice on the base
/// input). This is the tokenizer-facing entry: it never scans more than one
/// coarse block sequentially before handing control back to the index, so a
/// long run that merely *starts* after an earlier terminator in the same block
/// still gets the parallel benefit.
pub(crate) fn run_len_indexed(
    idx: &StructIndex,
    class: RunClass,
    remaining: &[u8],
    abs_off: usize,
) -> usize {
    let data_len = abs_off + remaining.len();
    let mut rel = 0usize;
    loop {
        let off = abs_off + rel;
        if off >= data_len {
            return rel;
        }
        if let Some(l) = idx.run_len(class, off, data_len) {
            return rel + l;
        }
        // The block's first terminator is before the cursor: scan only the
        // remainder of this coarse block sequentially, then resume the index.
        let block_end = (((off / COARSE) + 1) * COARSE).min(data_len);
        let end_rel = block_end - abs_off;
        let slice = &remaining[rel..end_rel];
        let local = match class {
            RunClass::Text => text_run_len_local(slice),
            RunClass::Comment => clean_run_len_local(slice, CleanClass::Comment),
            RunClass::Cdata => clean_run_len_local(slice, CleanClass::Cdata),
            RunClass::Pi => clean_run_len_local(slice, CleanClass::Pi),
        };
        if local < slice.len() {
            return rel + local;
        }
        rel = end_rel;
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §16.8.3 Level B — parallel UTF-8 validation
// ═════════════════════════════════════════════════════════════════════════════

/// Block size for the parallel UTF-8 validity pass. Coarser than [`BLOCK`]: the
/// validator is a cheap per-byte state walk and the boundary reconciliation is
/// O(blocks), so 64 KiB keeps the work balanced without a long sequential
/// checkpoint build.
pub(crate) const UTF8_BLOCK: usize = 64 * 1024;

/// §16.8.3 Level B — parallel lax UTF-8 validity with lead-byte-aligned block
/// boundaries.
///
/// This reproduces exactly the semantics of upstream `xmlCheckUTF8` (and the
/// candidate's faithful scalar `check_utf8`): a byte with the high bit clear is
/// a one-byte character; `110xxxxx`/`1110xxxx`/`11110xxx` lead a 2/3/4-byte
/// character whose following bytes must be continuations (`10xxxxxx`); any other
/// byte is invalid. It is a *shape* check (upstream does not reject overlong
/// forms, surrogates, or out-of-range code points here), and the parallel
/// decomposition must not strengthen or weaken it.
///
/// # Boundary reconciliation (§16.8.2/16.8.3)
///
/// A character must not straddle two blocks. The blocks are delimited by
/// **lead-byte positions only**: starting from a block boundary candidate, the
/// reconciler walks forward over continuation bytes (at most three) to the next
/// lead byte, so every block begins at the start of a character. Because a block
/// then never splits a character, each block can be validated independently, and
/// a truncated character at the end of a block is invalid — exactly as the
/// scalar walk sees a non-continuation byte (the NUL, or the next block's lead)
/// where a continuation was required.
pub(crate) fn utf8_is_valid_lax(bytes: &[u8]) -> bool {
    utf8_is_valid_lax_with(bytes, pool::config())
}

/// Like [`utf8_is_valid_lax`], with an explicit [`Config`] (used by the
/// differential tests to force the parallel path regardless of process state).
pub(crate) fn utf8_is_valid_lax_with(bytes: &[u8], cfg: &Config) -> bool {
    if !cfg.engages(bytes.len()) || cfg.threads <= 1 {
        return validate_lax_block(bytes);
    }
    // Lead-byte-aligned block starts (checkpoint build, O(blocks)).
    let mut starts: Vec<usize> = Vec::with_capacity(bytes.len() / UTF8_BLOCK + 2);
    starts.push(0);
    let mut s = 0usize;
    while s + UTF8_BLOCK < bytes.len() {
        let mut e = s + UTF8_BLOCK;
        while e < bytes.len() && bytes[e] & 0xc0 == 0x80 {
            e += 1;
        }
        if e >= bytes.len() {
            break;
        }
        starts.push(e);
        s = e;
    }
    let n = starts.len();
    pool::install(|| {
        (0..n).into_par_iter().all(|i| {
            let s = starts[i];
            let e = starts.get(i + 1).copied().unwrap_or(bytes.len());
            validate_lax_block(&bytes[s..e])
        })
    })
}

/// Validate one lead-byte-aligned slice under the exact `xmlCheckUTF8` shape
/// rules. A character that would need a byte beyond the slice is invalid (the
/// scalar walk would see a non-continuation byte there).
fn validate_lax_block(b: &[u8]) -> bool {
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c & 0x80 == 0 {
            i += 1;
        } else if c & 0xe0 == 0xc0 {
            if i + 1 >= b.len() || b[i + 1] & 0xc0 != 0x80 {
                return false;
            }
            i += 2;
        } else if c & 0xf0 == 0xe0 {
            if i + 2 >= b.len() || b[i + 1] & 0xc0 != 0x80 || b[i + 2] & 0xc0 != 0x80 {
                return false;
            }
            i += 3;
        } else if c & 0xf8 == 0xf0 {
            if i + 3 >= b.len()
                || b[i + 1] & 0xc0 != 0x80
                || b[i + 2] & 0xc0 != 0x80
                || b[i + 3] & 0xc0 != 0x80
            {
                return false;
            }
            i += 4;
        } else {
            // 10xxxxxx (a stray continuation), or 11111xxx — invalid.
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::pool::ParallelMode;
    use super::*;
    use crate::xml::parser::scan::scalar::scalar_text_run_len;

    /// Deterministic xorshift64 (no thread_rng: byte-for-byte reproducible).
    struct Xs(u64);
    impl Xs {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    fn forced() -> Config {
        Config {
            mode: ParallelMode::On,
            threads: 4,
            threshold: 0,
        }
    }

    fn scalar_first_special(b: &[u8]) -> Option<usize> {
        let r = scalar_text_run_len(b);
        (r < b.len()).then_some(r)
    }

    /// The parallel text scan is byte-identical to the scalar reference on
    /// adversarial inputs that cross every block/wave boundary.
    #[test]
    fn first_special_matches_scalar() {
        let cfg = forced();
        let mut rng = Xs(0x16_8_8_8);
        for iter in 0..2000u64 {
            let len = match iter % 8 {
                0 => (rng.next() % 70) as usize,
                1 => (rng.next() % 300) as usize,
                2 => (rng.next() % 5000) as usize,
                3 => BLOCK - 1 + (rng.next() % 3) as usize,
                4 => BLOCK + (rng.next() % 3) as usize,
                5 => 2 * BLOCK + (rng.next() % 3) as usize,
                6 => 3 * BLOCK - 1,
                _ => (rng.next() % 200_000) as usize,
            };
            let dist = iter % 7;
            let mut data = Vec::with_capacity(len);
            for i in 0..len {
                let b = match dist {
                    0 => (rng.next() & 0xFF) as u8,
                    1 => {
                        if (rng.next() & 0x3F) == 0 {
                            [b'<', b'&', b']', b'\n', b'\r'][(rng.next() % 5) as usize]
                        } else {
                            0x20 + (rng.next() % 0x5F) as u8
                        }
                    }
                    2 => {
                        if (rng.next() & 0x7FF) == 0 {
                            b'<'
                        } else {
                            b'a'
                        }
                    }
                    3 => {
                        if i + 1 == len {
                            b'<'
                        } else {
                            b'a'
                        }
                    }
                    4 => {
                        if i == len / 2 {
                            b'<'
                        } else {
                            b'x'
                        }
                    }
                    5 => b'a',
                    _ => {
                        if i % BLOCK == 0 {
                            b'&'
                        } else {
                            b'a'
                        }
                    }
                };
                data.push(b);
            }
            assert_eq!(
                first_special_traced(&data, &cfg).0,
                scalar_first_special(&data),
                "iter {iter} len {len} dist {dist}"
            );
        }
    }

    /// The class scanners match a scalar reference, including terminators one
    /// byte past a block boundary.
    #[test]
    fn clean_class_matches_scalar() {
        let classes = [CleanClass::Comment, CleanClass::Cdata, CleanClass::Pi];
        let mut rng = Xs(0x16_8_c1a5);
        for _ in 0..800 {
            let len = (rng.next() % (4 * BLOCK as u64)) as usize;
            let mut data = Vec::with_capacity(len);
            for _ in 0..len {
                data.push(match rng.next() % 6 {
                    0 => b'-',
                    1 => b']',
                    2 => b'?',
                    3 => b'\n',
                    4 => (rng.next() & 0xBF) as u8,
                    _ => b'a',
                });
            }
            for class in classes {
                let want = data
                    .iter()
                    .position(|&b| !class.is_clean(b))
                    .unwrap_or(data.len());
                assert_eq!(clean_run_len(&data, class), want, "len {len} {class:?}");
            }
        }
    }

    /// `first_byte` matches `slice::position` under the parallel path.
    #[test]
    fn first_byte_matches_scalar() {
        let cfg = forced();
        let mut rng = Xs(0x16_8_f00d);
        for _ in 0..500 {
            let len = (rng.next() % 400_000) as usize;
            let needle = (rng.next() & 0xFF) as u8;
            let mut data = vec![b'a'; len];
            if len > 0 && (rng.next() & 3) == 0 {
                let at = (rng.next() as usize) % len;
                data[at] = needle;
            }
            assert_eq!(
                first_byte_traced(&data, needle, &cfg).0,
                data.iter().position(|&x| x == needle)
            );
        }
    }

    /// The parallel decomposition is actually reached for a long clean run,
    /// and short runs stay on the zero-overhead sequential prefix.
    #[test]
    fn pool_is_engaged_only_for_long_runs() {
        let cfg = forced();
        let long = vec![b'a'; 8 * BLOCK];
        assert_eq!(first_special_traced(&long, &cfg), (None, true));
        let mut short = vec![b'a'; 4 * BLOCK];
        short[10] = b'<';
        assert_eq!(first_special_traced(&short, &cfg), (Some(10), false));
        let auto_high = Config {
            mode: ParallelMode::Auto,
            threads: 4,
            threshold: usize::MAX,
        };
        let clean = vec![b'a'; 4 * BLOCK];
        assert_eq!(first_special_traced(&clean, &auto_high), (None, false));
    }

    /// §16.8.7 isolation probe: time the parallel vs sequential scan on a large
    /// clean buffer. Run with `cargo test --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing probe"]
    fn timing_probe() {
        use std::time::Instant;
        let n = 64 * 1024 * 1024usize;
        let mut data = vec![b'a'; n];
        data[n - 1] = b'<';
        let seq = Config {
            mode: ParallelMode::Off,
            threads: 16,
            threshold: 0,
        };
        for threads in [2usize, 4, 8, 16] {
            let par = Config {
                mode: ParallelMode::On,
                threads,
                threshold: 0,
            };
            let mut best_s = f64::MAX;
            let mut best_p = f64::MAX;
            for _ in 0..5 {
                let t = Instant::now();
                let _ = first_special_traced(&data, &seq);
                best_s = best_s.min(t.elapsed().as_secs_f64());
                let t = Instant::now();
                let _ = first_special_traced(&data, &par);
                best_p = best_p.min(t.elapsed().as_secs_f64());
            }
            eprintln!(
                "scan {} MiB: seq {:.3} ms ({:.1} GB/s) par{} {:.3} ms ({:.1} GB/s) speedup {:.2}x",
                n >> 20,
                best_s * 1e3,
                (n as f64) / best_s / 1e9,
                threads,
                best_p * 1e3,
                (n as f64) / best_p / 1e9,
                best_s / best_p
            );
        }
        // Prepass cost: build the structural index for the same buffer.
        let cfg = forced();
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t = Instant::now();
            let _ = StructIndex::build_with(&data, &cfg);
            best = best.min(t.elapsed().as_secs_f64());
        }
        eprintln!(
            "index build {} MiB: {:.3} ms ({:.1} GB/s)",
            n >> 20,
            best * 1e3,
            (n as f64) / best / 1e9
        );
    }

    /// The structural index resolves every class/offset combination exactly.
    #[test]
    fn struct_index_matches_scalar() {
        let cfg = forced();
        let classes = [
            RunClass::Text,
            RunClass::Comment,
            RunClass::Cdata,
            RunClass::Pi,
        ];
        let mut rng = Xs(0x16_8_1de);
        let len = 5 * COARSE + 777;
        let mut data = Vec::with_capacity(len);
        for _ in 0..len {
            data.push(match rng.next() % 8 {
                0 => b'<',
                1 => b'-',
                2 => b']',
                3 => b'?',
                4 => b'\n',
                5 => (rng.next() & 0xFF) as u8,
                _ => b'a',
            });
        }
        let idx = StructIndex::build_with(&data, &cfg).expect("engages");
        for class in classes {
            for off in 0..data.len().min(3 * COARSE) {
                let want = data[off..]
                    .iter()
                    .position(|&b| class.is_terminator(b))
                    .unwrap_or(data.len() - off);
                assert_eq!(
                    idx.run_len(class, off, data.len())
                        .unwrap_or_else(|| data[off..]
                            .iter()
                            .position(|&b| class.is_terminator(b))
                            .unwrap_or(data.len() - off)),
                    want,
                    "{class:?} off {off}"
                );
            }
        }
    }

    /// `run_len_indexed` (the tokenizer-facing resolver) matches the scalar
    /// reference for every class and every start offset, including offsets
    /// after an earlier terminator in the same coarse block.
    #[test]
    fn run_len_indexed_matches_scalar() {
        let cfg = forced();
        let mut data = Vec::new();
        data.extend_from_slice(b"<p id=\"p0\">");
        data.extend(std::iter::repeat(b'x').take(BLOCK / 2 + 123));
        data.extend_from_slice(b"</p><!--");
        data.extend(std::iter::repeat(b'c').take(BLOCK / 2 + 7));
        data.extend_from_slice(b"--><![CDATA[");
        data.extend(std::iter::repeat(b'y').take(COARSE + 5));
        data.extend_from_slice(b"]]>");
        let idx = StructIndex::build_with(&data, &cfg).expect("engages");
        // Sample offsets (a small prefix, every coarse boundary and its
        // neighbours, plus a deterministic scatter) so the O(run) reference
        // scan stays cheap while still covering the in-block and cross-block
        // cases.
        let mut offs: Vec<usize> = (0..200.min(data.len())).collect();
        let mut o = 0;
        while o < data.len() {
            for d in [-1i64, 0, 1] {
                let x = o as i64 + d;
                if x >= 0 && (x as usize) < data.len() {
                    offs.push(x as usize);
                }
            }
            o += COARSE;
        }
        let mut rng = Xs(0x16_8_0ff5);
        for _ in 0..800 {
            offs.push((rng.next() as usize) % data.len());
        }
        offs.sort_unstable();
        offs.dedup();
        for class in [
            RunClass::Text,
            RunClass::Comment,
            RunClass::Cdata,
            RunClass::Pi,
        ] {
            for &off in &offs {
                let want = data[off..]
                    .iter()
                    .position(|&b| class.is_terminator(b))
                    .unwrap_or(data.len() - off);
                assert_eq!(
                    run_len_indexed(&idx, class, &data[off..], off),
                    want,
                    "{class:?} off {off}"
                );
            }
        }
    }

    /// The structural summary's totals equal a direct scalar count, and the
    /// prefix reconciliation is monotone and consistent.
    #[test]
    fn structural_summary_totals_and_reconciliation() {
        let cfg = forced();
        let mut rng = Xs(0x5eed_16_8);
        let len = 3 * BLOCK + 1234;
        let mut data = Vec::with_capacity(len);
        for _ in 0..len {
            data.push(match rng.next() % 5 {
                0 => b'<',
                1 => b'\n',
                2 => (rng.next() & 0xFF) as u8,
                3 => b'a',
                _ => 0xC3,
            });
        }
        let s = structural_summary_with(&data, &cfg);
        let direct_specials = data
            .iter()
            .filter(|&&b| !(0x20..=0x7E).contains(&b) || b == b'<' || b == b'&' || b == b']')
            .count() as u64;
        let direct_lb = data.iter().filter(|&&b| b == b'\n' || b == b'\r').count() as u64;
        let direct_nonascii = data.iter().filter(|&&b| b >= 0x80).count() as u64;
        assert_eq!(s.total_specials(), direct_specials);
        assert_eq!(s.total_non_ascii(), direct_nonascii);
        assert_eq!(s.total_linebreaks(), direct_lb);
        for i in 1..s.blocks.len() {
            assert!(s.special_at[i] >= s.special_at[i - 1]);
        }
    }

    /// Independent scalar reference for the lax `xmlCheckUTF8` shape semantics.
    fn lax_reference(b: &[u8]) -> bool {
        let mut i = 0usize;
        while i < b.len() {
            let c = b[i];
            let need = if c & 0x80 == 0 {
                1
            } else if c & 0xe0 == 0xc0 {
                2
            } else if c & 0xf0 == 0xe0 {
                3
            } else if c & 0xf8 == 0xf0 {
                4
            } else {
                return false;
            };
            if i + need > b.len() {
                return false;
            }
            for k in 1..need {
                if b[i + k] & 0xc0 != 0x80 {
                    return false;
                }
            }
            i += need;
        }
        true
    }

    /// §16.8.3 Level B: the parallel lead-byte-aligned validator is exactly the
    /// scalar `xmlCheckUTF8` shape check, including sequences that straddle
    /// every block boundary and truncated sequences at the end.
    #[test]
    fn utf8_validator_matches_scalar() {
        let cfg = forced();
        // Force the parallel path by using the same helper: it engages whenever
        // the config does, so drive it with a large-enough buffer below.
        let mut rng = Xs(0x16_8_4f8);
        // Deterministic character-level generation with multi-byte sequences
        // placed to cross every block boundary.
        for iter in 0..600u64 {
            let nblocks = 1 + (rng.next() % 4) as usize;
            let len = nblocks * UTF8_BLOCK + (rng.next() % 91) as usize;
            let mut data = Vec::with_capacity(len);
            while data.len() < len {
                let ch: Vec<u8> = match rng.next() % 8 {
                    0 => vec![b'a'],
                    1 => vec![0xC3, 0xA9],             // valid 2-byte
                    2 => vec![0xE2, 0x82, 0xAC],       // valid 3-byte (euro)
                    3 => vec![0xF0, 0x9F, 0x98, 0x80], // valid 4-byte
                    4 => vec![0x80],                   // stray continuation
                    5 => vec![0xC3],                   // truncated 2-byte
                    6 => vec![0xE2, 0x82],             // truncated 3-byte
                    _ => vec![0xF0, 0x9F],             // truncated 4-byte
                };
                if data.len() + ch.len() > len {
                    break;
                }
                data.extend_from_slice(&ch);
            }
            let want = lax_reference(&data);
            assert_eq!(
                utf8_is_valid_lax_with(&data, &cfg),
                want,
                "iter {iter} len {len} (parallel)"
            );
            // A pure-ASCII prefix is always valid; a lone trailing continuation
            // is always invalid.
            let mut ascii = vec![b'x'; len];
            assert!(utf8_is_valid_lax_with(&ascii, &cfg));
            if !ascii.is_empty() {
                let last = ascii.len() - 1;
                ascii[last] = 0x80;
                assert!(!utf8_is_valid_lax_with(&ascii, &cfg));
            }
        }
    }

    /// Truncated sequences at the very end of an exactly block-sized buffer
    /// must be invalid under both the scalar and parallel paths (the classic
    /// off-by-one boundary bug).
    #[test]
    fn utf8_truncated_at_block_boundary() {
        for off in 0..4usize {
            for ch_len in 2..=4usize {
                for got in 1..ch_len {
                    let mut data = vec![b'a'; UTF8_BLOCK * 2 - off + got];
                    let start = data.len() - got;
                    let lead = [0u8, 0xC3, 0xE2, 0xF0][ch_len - 1];
                    data[start] = lead;
                    // Fill the continuation bytes we do have with continuations.
                    for k in 1..got {
                        data[start + k] = 0x80;
                    }
                    // Now the final character is either complete (got == ch_len
                    // handled above by the loop bound) or truncated.
                    let want = lax_reference(&data);
                    assert_eq!(
                        utf8_is_valid_lax_with(&data, &forced()),
                        want,
                        "off {off} ch {ch_len} got {got}"
                    );
                }
            }
        }
    }
}
