#!/bin/sh
# par-oracle-run.sh — §16.8.8 oracle-relative matrix launcher (host side).
#
# Usage: sh courts/suites/phase16/par-oracle-run.sh [out-dir] [image]
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/16-8-oracle}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  docker run --rm -v "$OUT":/scanout "$IMAGE" bash -lc \
    'rm -rf /scanout/* /scanout/.[!.]* 2>/dev/null; exit 0' >/dev/null 2>&1 || true
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

{
  echo "candidate_sha=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "worktree_dirty=$( [ -n "$(git -C "$ROOT" status --porcelain 2>/dev/null)" ] && echo yes || echo no )"
  echo "image=$IMAGE"
  uname -a
  grep -m1 'model name' /proc/cpuinfo || true
} > "$OUT/run.txt"

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$ROOT/tools/bench":/bench:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/out \
  "$IMAGE" \
  bash /court/suites/phase16/par-oracle-matrix.sh 2>&1 | tee "$OUT/console.log"
