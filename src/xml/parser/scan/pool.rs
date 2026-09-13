//! §16.8.1 — private Rayon pool and diagnostic controls.
//!
//! libxml-rs is loaded *into* other applications (Python/lxml, Ruby/Nokogiri,
//! PHP, native binaries). It must never adopt or reconfigure the host's global
//! Rayon pool, because that would couple the library's parallelism to a
//! configuration it does not own and can change under it. This module therefore
//! builds a **private** [`rayon::ThreadPool`] lazily, on first eligible use, and
//! routes every parallel operation through `pool.install(...)`. The global pool
//! is never touched.
//!
//! # Diagnostic controls (not ABI)
//!
//! These environment variables exist for the §16.8 measurement court and for
//! operators diagnosing a workload; they are read once per process and are not
//! part of the C ABI:
//!
//! - `LIBXML_RS_PARALLEL=off|on|auto` — `off` forces the sequential path,
//!   `on` forces the parallel path (ignoring the size threshold), `auto`
//!   (default) uses the pool only above the measured crossover threshold.
//! - `LIBXML_RS_THREADS=N` — private-pool worker count (default: the number of
//!   available CPUs; `1` disables the pool).
//! - `LIBXML_RS_PARALLEL_THRESHOLD=BYTES` — overrides the `auto` crossover.
//!
//! # Fallback is not failure
//!
//! Every parallel entry point has an exact sequential equivalent and falls back
//! to it whenever the pool is disabled, the span is below the threshold, or the
//! pool cannot be built. An incorrect parallel result would be failure; a
//! fallback is the correct, honest outcome.

use std::sync::OnceLock;

/// Parallelism policy (`LIBXML_RS_PARALLEL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParallelMode {
    /// Always use the sequential scanner.
    Off,
    /// Always use the parallel scanner (for any non-empty eligible span).
    On,
    /// Use the parallel scanner only above [`Config::threshold`].
    Auto,
}

/// Resolved §16.8 configuration (read once per process).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Config {
    pub(crate) mode: ParallelMode,
    pub(crate) threads: usize,
    /// `auto` crossover: spans shorter than this stay sequential.
    pub(crate) threshold: usize,
}

/// Default `auto` crossover, established by the §16.8.7 measurement court.
///
/// This is the **cap** on the per-run escalation probe, not a constant budget:
/// the tokenizer probes `clamp(base_len / INDEX_BREAKEVEN_RATIO, BLOCK, threshold)`
/// bytes of a class run, so a small document probes only one block while a large
/// one probes up to this cap. Below the probe the run is resolved exactly with
/// no pool dispatch and no prepass; a run that reaches it has enough
/// class-scanner work to amortise the whole-input prepass. It is also the
/// minimum input size `auto` will parallelise at all.
///
/// The §16.8.7 sweep (`courts/receipts/phase-16/16-8-rayon.md`) measured 256 KiB
/// as the best cap: across 1 MiB–64 MiB single/many comment/CDATA shapes it
/// matched or beat 1 MiB and 4 MiB caps (e.g. 64 MiB one-CDATA 3.11x vs 2.54x),
/// never regressed below 1.0x, and — with the adaptive budget — still refuses to
/// build a 64 MiB prepass for a lone 100 KiB comment in a dense document.
pub(crate) const DEFAULT_THRESHOLD: usize = 256 * 1024;

/// Measured scalar-class-scan / structural-prepass throughput ratio (§16.8.7):
/// the sequential comment/CDATA classifier runs at `S ~ 2.4 GB/s` and the
/// one-load-per-byte prepass at `I ~ 27 GB/s`, so a run only justifies the
/// prepass once the *remaining* class-run work exceeds `(S/I) * len ~ len/11`.
/// The per-run probe budget is `len / INDEX_BREAKEVEN_RATIO` (capped), which is
/// exactly that break-even.
pub(crate) const INDEX_BREAKEVEN_RATIO: usize = 11;

/// Granularity of an independent parallel block (indexed chunking, §16.8.6).
pub(crate) const BLOCK: usize = 64 * 1024;

static CONFIG: OnceLock<Config> = OnceLock::new();
static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();

fn parse_mode() -> ParallelMode {
    match std::env::var("LIBXML_RS_PARALLEL") {
        Ok(v) => match v.to_ascii_lowercase().as_str() {
            "off" | "0" | "false" | "no" => ParallelMode::Off,
            "on" | "1" | "true" | "yes" => ParallelMode::On,
            _ => ParallelMode::Auto,
        },
        Err(_) => ParallelMode::Auto,
    }
}

fn parse_threads() -> usize {
    if let Ok(v) = std::env::var("LIBXML_RS_THREADS") {
        if let Ok(n) = v.trim().parse::<usize>() {
            return n.max(1);
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn parse_threshold() -> usize {
    if let Ok(v) = std::env::var("LIBXML_RS_PARALLEL_THRESHOLD") {
        if let Ok(n) = v.trim().parse::<usize>() {
            return n;
        }
    }
    DEFAULT_THRESHOLD
}

/// The resolved configuration (env read once).
pub(crate) fn config() -> &'static Config {
    CONFIG.get_or_init(|| Config {
        mode: parse_mode(),
        threads: parse_threads(),
        threshold: parse_threshold(),
    })
}

impl Config {
    /// Whether the pool should be engaged for a span of `len` bytes.
    pub(crate) fn engages(&self, len: usize) -> bool {
        match self.mode {
            ParallelMode::Off => false,
            ParallelMode::On => len > 0,
            ParallelMode::Auto => self.threads > 1 && len >= self.threshold,
        }
    }

    /// Whether the *per-call* ordered-wave scanner may engage the pool for a
    /// span of `len` bytes.
    ///
    /// `auto` deliberately keeps this off: the §16.8.7 measurements showed the
    /// per-call pool dispatch (~0.17 ms per wave set) is only amortised when it
    /// replaces a long *per-character* scan, which the whole-input structural
    /// index handles instead. `on` forces it for the measurement court.
    pub(crate) fn per_call_engages(&self, len: usize) -> bool {
        match self.mode {
            ParallelMode::Off => false,
            ParallelMode::On => len > 0,
            ParallelMode::Auto => false,
        }
    }
}

/// The private pool, or `None` when parallelism is disabled or the pool could
/// not be built. Built once, lazily, on first use.
pub(crate) fn pool() -> Option<&'static rayon::ThreadPool> {
    POOL.get_or_init(|| {
        let cfg = config();
        if cfg.mode == ParallelMode::Off || cfg.threads <= 1 {
            return None;
        }
        rayon::ThreadPoolBuilder::new()
            .num_threads(cfg.threads)
            .thread_name(|i| format!("libxml-rs-par-{i}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// Run `f` on the private pool and return its result.
///
/// The caller must have established (via [`Config::engages`]) that the work
/// warrants the pool. The global Rayon pool is never used.
pub(crate) fn install<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    match pool() {
        Some(p) => p.install(f),
        None => f(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_stable_and_positive() {
        let c = config();
        assert!(c.threads >= 1);
        assert!(c.threshold > 0);
        // Repeated reads return the same cached instance.
        assert!(core::ptr::eq(c, config()));
    }

    #[test]
    fn engages_respects_mode_and_threshold() {
        let mk = |mode, threads, threshold| Config {
            mode,
            threads,
            threshold,
        };
        assert!(!mk(ParallelMode::Off, 8, 1).engages(usize::MAX));
        assert!(mk(ParallelMode::On, 8, usize::MAX).engages(1));
        assert!(!mk(ParallelMode::Auto, 1, 1).engages(usize::MAX));
        assert!(!mk(ParallelMode::Auto, 8, 1024).engages(1023));
        assert!(mk(ParallelMode::Auto, 8, 1024).engages(1024));
    }

    #[test]
    fn install_falls_back_without_pool() {
        // With `off`/single-thread the pool is None and `install` runs inline.
        // (Read-only check: never mutates global config.)
        let _ = pool();
        let v = install(|| 7usize);
        assert_eq!(v, 7);
    }
}
