#!/usr/bin/env python3
"""Oracle-vs-candidate Pareto matrix for the libxml-rs drop-in.

Compiles the single C harness (tools/bench/harness.c) against BOTH the
oracle (upstream libxml2 + libxslt) and the candidate (libxml-rs three-DSO
facades), runs an identical grid of (operation, input-size), and reports a
matrix of mean latency, throughput, and the candidate speedup vs the oracle.

The harness exercises the public C ABI only, so both sides run the exact same
code path and the comparison is apples-to-apples.

Environment (all optional; sensible container defaults below):
  ORACLE_PREFIX   prefix for the upstream install        (default /usr/local)
  CANDIDATE_PREFIX prefix for the candidate artifacts     (default /candidate)
  OUTPUT          where to write the matrix                (default ./bench-matrix)

Run inside a minimal Docker VM (see run_in_docker.sh); the host build may not
have the oracle + candidate prefixes present.
"""

import csv
import io
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS = os.path.join(HERE, "harness.c")

ORACLE = os.environ.get("ORACLE_PREFIX", "/usr/local")
CANDIDATE = os.environ.get("CANDIDATE_PREFIX", "/candidate")
OUT = os.environ.get("OUTPUT", os.path.join(HERE, "bench-matrix"))

# operation -> approximate input sizes (bytes)
GRID = {
    "parse": [1024, 16384, 131072],
    "xpath": [1024, 16384],
    "serialize": [1024, 16384],
    "html": [1024, 16384],
    "validate": [1024, 16384],
    "xslt": [256, 1024],
}

# Each (target, iterations) is fixed for both sides so wall-time differences
# are a true speedup, not a sampling artifact. Kept modest so the matrix
# completes in a CI-bounded time; the candidate-side statistical rigor lives
# in the Criterion benchmarks, this harness is the oracle comparison.
ITERS = {
    256: 20,
    1024: 20,
    16384: 10,
    131072: 3,
}


def compile_harness(prefix, out_path, name):
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


def run(harness_bin, op, size, iters):
    p = subprocess.run(
        [harness_bin, op, str(size), str(iters)],
        capture_output=True, text=True, check=True,
    )
    last = p.stdout.strip().splitlines()[-1]
    op_name, bytes_str, iters_str, mean_ns, thrpt = last.split(",")
    return {
        "op": op_name,
        "bytes": int(bytes_str),
        "iters": int(iters_str),
        "mean_ns": float(mean_ns),
        "thrpt_bytes_per_sec": float(thrpt),
    }


def human_bytes(n):
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or unit == "GiB":
            return f"{n:.1f}{unit}" if unit != "B" else f"{int(n)}B"
        n /= 1024.0


def human_ns(ns):
    if ns < 1e3:
        return f"{ns:.1f}ns"
    if ns < 1e6:
        return f"{ns/1e3:.2f}µs"
    if ns < 1e9:
        return f"{ns/1e6:.3f}ms"
    return f"{ns/1e9:.3f}s"


def main():
    os.makedirs(OUT, exist_ok=True)
    oracle_bin = os.path.join(OUT, "harness-oracle")
    cand_bin = os.path.join(OUT, "harness-candidate")
    compile_harness(ORACLE, oracle_bin, "oracle")
    compile_harness(CANDIDATE, cand_bin, "candidate")

    rows = []
    for op in sorted(GRID):
        for size in GRID[op]:
            iters = ITERS.get(size, 100)
            o = run(oracle_bin, op, size, iters)
            c = run(cand_bin, op, size, iters)
            speedup = o["mean_ns"] / c["mean_ns"] if c["mean_ns"] > 0 else float("inf")
            rows.append((op, size, o, c, speedup))

    # JSON for CI / tooling.
    payload = {
        "schema": "bench-pareto-matrix/1",
        "oracle": ORACLE,
        "candidate": CANDIDATE,
        "rows": [
            {
                "op": op, "bytes": size,
                "oracle_mean_ns": o["mean_ns"],
                "candidate_mean_ns": c["mean_ns"],
                "oracle_thrpt": o["thrpt_bytes_per_sec"],
                "candidate_thrpt": c["thrpt_bytes_per_sec"],
                "speedup": speedup,
            }
            for op, size, o, c, speedup in rows
        ],
    }
    with open(os.path.join(OUT, "matrix.json"), "w") as f:
        json.dump(payload, f, indent=2)

    # Markdown Pareto matrix.
    md = io.StringIO()
    md.write("# libxml-rs vs upstream libxml2 — Pareto matrix\n\n")
    md.write("Lower latency / higher throughput is better. `speedup` > 1 means the\n")
    md.write("candidate is faster; < 1 means the oracle is faster.\n\n")
    md.write("| op | input | oracle latency | candidate latency | speedup | candidate throughput |\n")
    md.write("|---|---|---|---|---|---|\n")
    for op, size, o, c, speedup in rows:
        md.write(
            f"| {op} | {human_bytes(size)} | {human_ns(o['mean_ns'])} | "
            f"{human_ns(c['mean_ns'])} | {speedup:.3f}× | "
            f"{human_bytes(c['thrpt_bytes_per_sec'])}/s |\n"
        )
    with open(os.path.join(OUT, "matrix.md"), "w") as f:
        f.write(md.getvalue())

    print(f"wrote {os.path.join(OUT, 'matrix.md')} + matrix.json")
    print(md.getvalue())
    return 0


if __name__ == "__main__":
    sys.exit(main())
