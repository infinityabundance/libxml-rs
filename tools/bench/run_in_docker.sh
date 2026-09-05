#!/bin/sh
# run_in_docker.sh — run the oracle-vs-candidate Pareto matrix in a minimal
# Docker VM.
#
# The `phase14-debian:1` image ships the source-built upstream oracle
# (libxml2 2.15.3 + libxslt 1.1.45) at /usr/local and a C toolchain, so the
# harness compiles against both sides with the same flags. The candidate
# three-DSO artifacts are mounted read-only at /candidate from the host's
# target/release (the optimized profile — run `cargo build --release --lib`
# first, then `sh tools/packaging/facade-gen.sh target/release`).
#
# Usage:  sh tools/bench/run_in_docker.sh [output-dir]

set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/target/bench-matrix}"
# Canonicalize to an absolute path so docker -v treats it as a host dir.
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
docker run --rm \
  -v "$ROOT/tools/bench":/bench:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/bench-out \
  -e ORACLE_PREFIX=/usr/local \
  -e CANDIDATE_PREFIX=/candidate \
  -e OUTPUT=/bench-out \
  libxml-rs/phase14-debian:1 \
  python3 /bench/pareto_matrix.py

echo "matrix written to $OUT"
