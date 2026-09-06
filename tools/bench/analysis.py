#!/usr/bin/env python3
"""analysis.py — dependency-free statistical layer for the Phase 16 performance court.

This is the single source of truth for:

* central-tendency / dispersion statistics (median, mean, sample stddev, MAD,
  percentiles) without numpy/scipy,
* bootstrap confidence intervals,
* the speedup ratio and its bootstrap CI,
* the verdict classification (SIGNIFICANT_WIN / PRACTICAL_WIN / TIE /
  PRACTICAL_REGRESSION / SIGNIFICANT_REGRESSION),
* geometric-mean aggregation.

It consumes the raw observation lists produced by the provider-isolated harness
and the five-consumer drivers, and is deliberately stdlib-only so it runs in the
minimal Debian performance image (which has no numpy/scipy).
"""

import json
import math
import random
from dataclasses import dataclass, field
from typing import Iterable, List, Optional, Sequence, Tuple

# ── Classification thresholds (§16.2 / PERFORMANCE_ATLAS) ──────────────────
EXPLORATORY_CONFIDENCE = 0.95
FINAL_CONFIDENCE = 0.99
PRACTICAL_NOISE_RATIO = 1.05
BOOTSTRAP_RESAMPLES = 10_000
BOOTSTRAP_SEED = 0xC0FFEE


def mean(xs: Sequence[float]) -> float:
    if not xs:
        return float("nan")
    return sum(xs) / len(xs)


def median(xs: Sequence[float]) -> float:
    if not xs:
        return float("nan")
    s = sorted(xs)
    n = len(s)
    if n % 2:
        return s[n // 2]
    return (s[n // 2 - 1] + s[n // 2]) / 2.0


def sample_stddev(xs: Sequence[float]) -> float:
    if len(xs) < 2:
        return float("nan")
    m = mean(xs)
    var = sum((x - m) ** 2 for x in xs) / (len(xs) - 1)
    return math.sqrt(var)


def mad(xs: Sequence[float]) -> float:
    """Median absolute deviation (scaled for normal consistency by 1.4826)."""
    if not xs:
        return float("nan")
    m = median(xs)
    devs = [abs(x - m) for x in xs]
    return 1.4826 * median(devs)


def percentile(xs: Sequence[float], q: float) -> float:
    """Linear-interpolation percentile (matches numpy's default)."""
    if not xs:
        return float("nan")
    s = sorted(xs)
    n = len(s)
    if n == 1:
        return s[0]
    rank = q * (n - 1)
    lo = int(math.floor(rank))
    hi = int(math.ceil(rank))
    if lo == hi:
        return s[lo]
    frac = rank - lo
    return s[lo] * (1.0 - frac) + s[hi] * frac


def geomean(xs: Sequence[float]) -> float:
    if not xs:
        return float("nan")
    logs = [math.log(x) for x in xs if x > 0]
    if not logs:
        return float("nan")
    return math.exp(sum(logs) / len(logs))


@dataclass
class Summary:
    """Descriptive statistics over a list of latency observations (ns)."""
    n: int
    median_ns: float
    mean_ns: float
    stddev_ns: float
    mad_ns: float
    p50_ns: float
    p95_ns: float
    p99_ns: float

    @staticmethod
    def from_samples(samples: Sequence[float]) -> "Summary":
        return Summary(
            n=len(samples),
            median_ns=median(samples),
            mean_ns=mean(samples),
            stddev_ns=sample_stddev(samples),
            mad_ns=mad(samples),
            p50_ns=percentile(samples, 0.50),
            p95_ns=percentile(samples, 0.95),
            p99_ns=percentile(samples, 0.99),
        )


def bootstrap_ci(
    samples: Sequence[float],
    level: float,
    n_boot: int = BOOTSTRAP_RESAMPLES,
    seed: int = BOOTSTRAP_SEED,
) -> Tuple[float, float]:
    """Percentile bootstrap CI on the MEAN of `samples`."""
    if not samples:
        return (float("nan"), float("nan"))
    if len(samples) == 1:
        return (samples[0], samples[0])
    rng = random.Random(seed)
    n = len(samples)
    means: List[float] = []
    for _ in range(n_boot):
        resampled = [samples[rng.randrange(n)] for _ in range(n)]
        means.append(sum(resampled) / n)
    means.sort()
    alpha = 1.0 - level
    lo_idx = int(alpha / 2 * n_boot)
    hi_idx = int((1.0 - alpha / 2) * n_boot) - 1
    hi_idx = max(lo_idx, min(hi_idx, n_boot - 1))
    return (means[lo_idx], means[hi_idx])


def speedup_ratio(
    oracle_samples: Sequence[float],
    candidate_samples: Sequence[float],
) -> float:
    """oracle_mean / candidate_mean (see PERFORMANCE_ATLAS convention)."""
    om, cm = mean(oracle_samples), mean(candidate_samples)
    if cm <= 0:
        return float("inf")
    return om / cm


def speedup_bootstrap_ci(
    oracle_samples: Sequence[float],
    candidate_samples: Sequence[float],
    level: float,
    n_boot: int = BOOTSTRAP_RESAMPLES,
    seed: int = BOOTSTRAP_SEED,
) -> Tuple[float, float]:
    """Percentile bootstrap CI on the ratio oracle_mean / candidate_mean.

    Oracle and candidate samples are resampled independently so the CI reflects
    the joint sampling uncertainty of the ratio.
    """
    if not oracle_samples or not candidate_samples:
        return (float("nan"), float("nan"))
    rng = random.Random(seed)
    no, nc = len(oracle_samples), len(candidate_samples)
    ratios: List[float] = []
    for _ in range(n_boot):
        o = [oracle_samples[rng.randrange(no)] for _ in range(no)]
        c = [candidate_samples[rng.randrange(nc)] for _ in range(nc)]
        om = sum(o) / no
        cm = sum(c) / nc
        ratios.append(om / cm if cm > 0 else float("inf"))
    ratios.sort()
    alpha = 1.0 - level
    lo_idx = int(alpha / 2 * n_boot)
    hi_idx = int((1.0 - alpha / 2) * n_boot) - 1
    hi_idx = max(lo_idx, min(hi_idx, n_boot - 1))
    return (ratios[lo_idx], ratios[hi_idx])


def classify_ratio(ratio_ci: Tuple[float, float], level: float) -> str:
    """Mechanical verdict from a bootstrap CI on the speedup ratio.

    The practical-noise band is [1/PRACTICAL_NOISE_RATIO, PRACTICAL_NOISE_RATIO].
    A CI that overlaps that band is a TIE regardless of its point estimate.
    """
    lo, hi = ratio_ci
    lower = 1.0 / PRACTICAL_NOISE_RATIO
    upper = PRACTICAL_NOISE_RATIO
    if math.isnan(lo) or math.isnan(hi):
        return "TIE"
    if lo >= upper:
        return "SIGNIFICANT_WIN" if level >= FINAL_CONFIDENCE else "PRACTICAL_WIN"
    if hi <= lower:
        return "SIGNIFICANT_REGRESSION" if level >= FINAL_CONFIDENCE else "PRACTICAL_REGRESSION"
    return "TIE"


def load_json_rows(path: str) -> List[dict]:
    with open(path) as f:
        data = json.load(f)
    if isinstance(data, list):
        return data
    if isinstance(data, dict) and "rows" in data:
        return data["rows"]
    raise ValueError(f"{path}: expected a list or {{'rows': [...]}}")


def dump_json(obj, path: str) -> None:
    with open(path, "w") as f:
        json.dump(obj, f, indent=2)
        f.write("\n")


def summarize_pair(
    oracle_samples: Sequence[float],
    candidate_samples: Sequence[float],
    level: float = EXPLORATORY_CONFIDENCE,
) -> dict:
    """One-stop descriptive + ratio + CI + verdict for a provider pair."""
    os_, cs = Summary.from_samples(oracle_samples), Summary.from_samples(candidate_samples)
    ratio = speedup_ratio(oracle_samples, candidate_samples)
    ci = speedup_bootstrap_ci(oracle_samples, candidate_samples, level)
    verdict = classify_ratio(ci, level)
    return {
        "oracle": os_.__dict__,
        "candidate": cs.__dict__,
        "speedup": ratio,
        "speedup_ci": {"level": level, "lower": ci[0], "upper": ci[1]},
        "verdict": verdict,
    }


if __name__ == "__main__":
    # Self-check: a clearly-faster candidate should classify as a win.
    o = [100.0, 101.0, 99.0, 102.0, 100.0, 101.0, 100.0]
    c = [50.0, 51.0, 49.0, 50.0, 52.0, 50.0, 51.0]
    print(summarize_pair(o, c))
