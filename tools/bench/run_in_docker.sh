#!/bin/sh
# run_in_docker.sh — run the oracle-vs-candidate Pareto matrix (Phase 16.2) in
# a minimal Docker VM.
#
# The `phase14-debian:1` image ships the source-built upstream oracle
# (libxml2 2.15.3 + libxslt 1.1.45) at /usr/local and a C toolchain, so the
# harness compiles against both sides with the same flags. The candidate
# three-DSO artifacts are mounted read-only at /candidate from the host's
# target/release (run `cargo build --release --lib` first, then
# `sh tools/packaging/facade-gen.sh target/release`).
#
# Usage:
#   sh tools/bench/run_in_docker.sh [output-dir] [cpuset]
#
# Env overrides: BENCH_TRIALS BENCH_MIN_NS BENCH_CONFIDENCE
#   BENCH_IMAGE — oracle court image tag (default libxml-rs/phase14-debian:1,
#   the locally built court; CI builds the same oracle from
#   docker/Dockerfile.oracle and passes that tag).

set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/target/bench-matrix}"
CPUSET="${2:-0}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"
# Canonicalize to an absolute path so docker -v treats it as a host dir.
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"

docker run --rm \
  --cpuset-cpus="$CPUSET" \
  -v "$ROOT/tools/bench":/bench:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/bench-out \
  -e ORACLE_PREFIX=/usr/local \
  -e CANDIDATE_PREFIX=/candidate \
  -e OUTPUT=/bench-out \
  -e BENCH_TRIALS="${BENCH_TRIALS:-7}" \
  -e BENCH_MIN_NS="${BENCH_MIN_NS:-50000000}" \
  -e BENCH_CPUSET="$CPUSET" \
  -e BENCH_CONFIDENCE="${BENCH_CONFIDENCE:-0.95}" \
  -e CANDIDATE_SHA="$CANDIDATE_SHA" \
  "$IMAGE" \
  python3 /bench/pareto_matrix.py

# Retain raw observations in the forensic evidence tree.
mkdir -p "$ROOT/courts/receipts/phase-16/raw"
cp -f "$OUT/raw.json" "$ROOT/courts/receipts/phase-16/raw/pareto-$(date +%Y%m%dT%H%M%S).json" 2>/dev/null || true

echo "matrix written to $OUT"
