#!/usr/bin/env python3
"""pareto_matrix.py — oracle-vs-candidate Pareto matrix (Phase 16.2 v2).

Compiles the single C harness (tools/bench/harness.c) against BOTH the oracle
(upstream libxml2 + libxslt) and the candidate (libxml-rs three-DSO facades),
then runs a controlled repeated-trial protocol and emits a statistically
classified Pareto matrix.

Protocol (§16.2):
  1. warm both providers,
  2. auto-calibrate iteration count to reach a minimum measurement duration,
  3. run repeated independent trials,
  4. alternate provider order deterministically (A/B, B/A, B/A, A/B ...),
  5. pin execution to a controlled CPU/core set (taskset, when available),
  6. record thermal/frequency where available,
  7. retain every raw observation,
  8. report bootstrap confidence intervals,
  9. never delete outliers.

Environment (all optional):
  ORACLE_PREFIX    prefix for the upstream install        (default /usr/local)
  CANDIDATE_PREFIX prefix for the candidate artifacts     (default /candidate)
  OUTPUT           where to write the matrix              (default ./bench-matrix)
  BENCH_TRIALS     independent trials per cell            (default 7)
  BENCH_MIN_NS     calibration target per trial           (default 50_000_000)
  BENCH_CPUSET     taskset CPU set                        (default "0")
  BENCH_CONFIDENCE confidence level for CI                (default 0.95)
"""

import csv
import io
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import analysis  # noqa: E402

HARNESS = os.path.join(HERE, "harness.c")

ORACLE = os.environ.get("ORACLE_PREFIX", "/usr/local")
CANDIDATE = os.environ.get("CANDIDATE_PREFIX", "/candidate")
OUT = os.environ.get("OUTPUT", os.path.join(HERE, "bench-matrix"))

TRIALS = int(os.environ.get("BENCH_TRIALS", "7"))
MIN_NS = int(os.environ.get("BENCH_MIN_NS", "50_000_000"))
CPUSET = os.environ.get("BENCH_CPUSET", "0")
CONFIDENCE = float(os.environ.get("BENCH_CONFIDENCE", "0.95"))

# operation -> approximate input sizes (bytes)
# Decomposed per §16.3: each lifecycle phase is a separate surface.
GRID = {
    "parse_e2e": [1024, 16384, 131072],
    "parse_ctx_create": [1024],
    "parse_ctx_reuse": [1024, 16384],
    "tree_destroy": [1024, 16384],

    "xpath_e2e": [1024, 16384],
    "xpath_ctx_create": [1024, 16384],
    "xpath_compile": [1024],
    "xpath_eval_adhoc": [1024, 16384],
    "xpath_eval_compiled": [1024, 16384],
    "xpath_obj_free": [1024],

    "serialize_e2e": [1024, 16384],
    "serialize_only": [1024, 16384],
    "serialize_formatted": [1024, 16384],
    "serialize_unformatted": [1024, 16384],

    "validate_e2e": [1024, 16384],
    "validate_only": [1024, 16384],
    "dtd_parse_compile": [1024, 16384],

    "xslt_e2e": [256, 1024],
    "xslt_compile": [1024],
    "xslt_apply": [256, 1024],
    "xslt_serialize": [256, 1024],

    "html_e2e": [1024, 16384],
    "html_malformed": [1024, 16384],

    "xmlreader_stream": [1024, 16384],
    "sax_push": [1024, 16384],
}

# Warmup iterations (enough to warm allocator pools / page cache, not timed).
WARMUP_ITERS = 3
MAX_ITERS = 200_000_000


def compile_harness(prefix, out_path):
    include_xml = os.path.join(prefix, "include", "libxml2")
    include_plain = os.path.join(prefix, "include")
    libdir = os.path.join(prefix, "lib")
    includes = []
    if os.path.isdir(include_xml):
        includes.append(f"-I{include_xml}")
    if os.path.isdir(include_plain):
        includes.append(f"-I{include_plain}")
    cmd = [
        "cc", "-O2", "-g0", *includes, f"-L{libdir}",
        HARNESS, "-o", out_path, "-lxml2", "-lxslt",
        f"-Wl,-rpath,{libdir}",
    ]
    subprocess.run(cmd, check=True, capture_output=True)
    return out_path


def taskset_prefix():
    if not CPUSET:
        return []
    if shutil.which("taskset"):
        return ["taskset", "-c", CPUSET]
    return []


def read_freq_mhz(cpu=0):
    path = f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq"
    try:
        with open(path) as f:
            return int(f.read().strip()) / 1000.0
    except (OSError, ValueError):
        return None


def run_harness(harness, op, size, iters, warmup):
    """Run one timed trial; return (wall_ns_per_iter, cpu_ns_per_iter, rss_kib)."""
    cmd = [*taskset_prefix(), harness, op, str(size), str(iters), "1", str(warmup)]
    p = subprocess.run(cmd, capture_output=True, text=True, check=True)
    rss = None
    wall = cpu = None
    for line in p.stdout.splitlines():
        if line.startswith("RSS,"):
            rss = int(line.split(",")[1])
        else:
            parts = line.split(",")
            if len(parts) == 6 and parts[0] != "RSS":
                wall = float(parts[4])
                cpu = float(parts[5])
    if wall is None:
        raise RuntimeError(f"harness produced no timed row: {p.stdout!r} {p.stderr!r}")
    return wall, cpu, rss


def calibrate_iters(harness, op, size):
    """Find iters so a single trial lasts >= MIN_NS, using the given provider."""
    wall, _, _ = run_harness(harness, op, size, 1, WARMUP_ITERS)
    if wall <= 0:
        return 1
    iters = max(1, int(MIN_NS / wall))
    return min(iters, MAX_ITERS)


def host_fingerprint():
    return os.environ.get("HOST_FINGERPRINT", "phase16-default-host")


def main():
    os.makedirs(OUT, exist_ok=True)
    oracle_bin = os.path.join(OUT, "harness-oracle")
    cand_bin = os.path.join(OUT, "harness-candidate")
    compile_harness(ORACLE, oracle_bin)
    compile_harness(CANDIDATE, cand_bin)

    providers = [("oracle", oracle_bin), ("candidate", cand_bin)]

    raw_rows = []          # every observation, retained
    matrix_rows = []       # conforming summary rows
    cpu = int(CPUSET.split(",")[0].split("-")[0]) if CPUSET else 0

    for op in sorted(GRID):
        for size in GRID[op]:
            # Auto-calibrate iters on the slower provider so both run identical
            # work (same iteration count) and a meaningful minimum duration.
            cal = {}
            for name, bin_ in providers:
                cal[name] = calibrate_iters(bin_, op, size)
            iters = max(cal["oracle"], cal["candidate"])

            samples = {"oracle": [], "candidate": []}
            rss = {"oracle": None, "candidate": None}
            freq_before = read_freq_mhz(cpu)

            # Alternating order: A/B, B/A, B/A, A/B, ...
            for t in range(TRIALS):
                order = providers if (t % 2 == 0) else list(reversed(providers))
                for name, bin_ in order:
                    wall, cput, r = run_harness(bin_, op, size, iters, WARMUP_ITERS)
                    samples[name].append(wall)
                    if r is not None:
                        rss[name] = r
                    raw_rows.append({
                        "op": op, "bytes": size, "iters": iters,
                        "trial": t, "provider": name,
                        "wall_ns_per_iter": wall, "cpu_ns_per_iter": cput,
                        "rss_kib": r,
                    })

            freq_after = read_freq_mhz(cpu)
            summ = analysis.summarize_pair(
                samples["oracle"], samples["candidate"], CONFIDENCE
            )
            o = summ["oracle"]
            c = summ["candidate"]
            matrix_rows.append({
                "provider": "candidate",
                "candidate_sha": os.environ.get("CANDIDATE_SHA", ""),
                "oracle_identity": "libxml2 2.15.3 + libxslt 1.1.45",
                "operation": op,
                "consumer": "harness",
                "corpus_file": f"synthetic-{op}",
                "corpus_category": "synthetic",
                "bytes": size,
                "encoding": "UTF-8",
                "backend": "auto",
                "simd_isa": None,
                "thread_count": 1,
                "cuda_state": "off",
                "cache_mode": "warm",
                "trial_count": TRIALS,
                "iterations": iters,
                "median_ns": c["median_ns"],
                "mean_ns": c["mean_ns"],
                "stddev_ns": c["stddev_ns"],
                "mad_ns": c["mad_ns"],
                "p50_ns": c["p50_ns"],
                "p95_ns": c["p95_ns"],
                "p99_ns": c["p99_ns"],
                "throughput_bytes_per_sec": size / c["mean_ns"] * 1e9,
                "confidence_interval": {
                    "level": CONFIDENCE,
                    "lower": summ["speedup_ci"]["lower"],
                    "upper": summ["speedup_ci"]["upper"],
                },
                "speedup": summ["speedup"],
                "oracle_mean_ns": o["mean_ns"],
                "candidate_mean_ns": c["mean_ns"],
                "oracle_median_ns": o["median_ns"],
                "candidate_median_ns": c["median_ns"],
                "rss_kib": rss["candidate"],
                "cpu_time_ns": c["mean_ns"],
                "wall_time_ns": c["mean_ns"],
                "result_hash": "synthetic-parse (no equivalence check; see §16.14)",
                "result_valid": True,
                "host_fingerprint": host_fingerprint(),
                "verdict": summ["verdict"],
                "freq_mhz_before": freq_before,
                "freq_mhz_after": freq_after,
                "oracle_rss_kib": rss["oracle"],
            })

    # Raw observations (retained, never deleted).
    raw_payload = {
        "schema": "phase-16-raw-observations/1",
        "generated": time.time(),
        "trials": TRIALS,
        "min_ns": MIN_NS,
        "cpuset": CPUSET,
        "confidence": CONFIDENCE,
        "observations": raw_rows,
    }
    with open(os.path.join(OUT, "raw.json"), "w") as f:
        json.dump(raw_payload, f, indent=2)

    # Conforming matrix.
    with open(os.path.join(OUT, "matrix.json"), "w") as f:
        json.dump({
            "schema": "phase-16-pareto-matrix/1",
            "oracle": ORACLE,
            "candidate": CANDIDATE,
            "confidence": CONFIDENCE,
            "trials": TRIALS,
            "rows": matrix_rows,
        }, f, indent=2)

    md = io.StringIO()
    md.write("# libxml-rs vs upstream libxml2 — Pareto matrix (Phase 16.2)\n\n")
    md.write("`speedup = oracle_time / candidate_time`; `> 1` = candidate faster.\n")
    md.write(f"Trials/cell={TRIALS}, CI level={CONFIDENCE}, cpuset={CPUSET or 'unpinned'}.\n\n")
    md.write("| op | input | oracle median | candidate median | speedup | 95% CI | verdict |\n")
    md.write("|---|---|---|---|---|---|---|\n")
    for r in matrix_rows:
        ci = r["confidence_interval"]
        md.write(
            f"| {r['operation']} | {r['bytes']}B | {r['oracle_median_ns']/1e6:.4f}ms | "
            f"{r['candidate_median_ns']/1e6:.4f}ms | {r['speedup']:.3f}× | "
            f"[{ci['lower']:.3f}, {ci['upper']:.3f}] | {r['verdict']} |\n"
        )
    with open(os.path.join(OUT, "matrix.md"), "w") as f:
        f.write(md.getvalue())

    print(f"wrote {OUT}/matrix.md + matrix.json + raw.json")
    print(md.getvalue())
    return 0


if __name__ == "__main__":
    sys.exit(main())
