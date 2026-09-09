#!/bin/sh
# pushdiff-run.sh — §16.7.8 push-parser per-call differential court (host
# launcher). Runs pushdiff-differential.sh inside the oracle court VM over
# the adversarial push corpus + phase-14 fixtures and reports every
# (document, chunking-plan) cell whose oracle trace differs from the
# candidate trace, byte-for-byte.
#
# Usage: sh courts/suites/phase16/pushdiff-run.sh [out-dir] [image]
# Env: BENCH_IMAGE default libxml-rs/phase14-debian:1. Requires a release
# build: cargo build --release --lib && sh tools/packaging/facade-gen.sh
# target/release (so /candidate carries the current engine).
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/pushdiff}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

# Files created by earlier docker runs are root-owned; clean through docker.
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  docker run --rm -v "$OUT":/scanout "$IMAGE" bash -lc 'rm -rf /scanout/* /scanout/.[!.]* 2>/dev/null; exit 0' >/dev/null 2>&1 || true
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
echo "candidate_sha=$CANDIDATE_SHA" > "$OUT/run.txt"
echo "image=$IMAGE" >> "$OUT/run.txt"
uname -a >> "$OUT/run.txt" 2>/dev/null || true
grep -m1 'model name' /proc/cpuinfo >> "$OUT/run.txt" || true

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/scanout \
  "$IMAGE" \
  bash /court/suites/phase16/pushdiff-differential.sh 2>&1 | tee "$OUT/console.log"
