#!/bin/sh
# par-run.sh — §16.8 parallel-blocking differential launcher (host side).
#
# Runs courts/suites/phase16/par-differential.sh inside the oracle court VM
# against the release candidate DSOs, writing raw outputs + console log to
# courts/receipts/phase-16/raw/16-8-differential/.
#
# Requires a release build: cargo build --release --lib &&
# sh tools/packaging/facade-gen.sh target/release
#
# Usage: sh courts/suites/phase16/par-run.sh [out-dir] [image]
# Env: BENCH_IMAGE default libxml-rs/phase14-debian:1.
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/16-8-differential}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

# Capture the source state BEFORE the cleanup below deletes tracked evidence
# files. The raw evidence tree is excluded from the seal: it is the *output*
# of the court, so regenerating it must not mark the source under test dirty.
CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
WORKTREE_DIRTY="$( [ -n "$(git -C "$ROOT" status --porcelain -- . ':(exclude)courts/receipts/phase-16/raw' 2>/dev/null)" ] && echo yes || echo no )"

# Files created by earlier docker runs are root-owned; clean through docker.
# The tracked `.gitignore` is PRESERVED (deleting it would un-ignore the raw
# per-cell outputs and the next `git add -A` would commit the large dumps).
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  docker run --rm -v "$OUT":/parout "$IMAGE" bash -lc \
    'cd /parout; for f in * .[!.]*; do [ "$f" = ".gitignore" ] || rm -rf -- "$f"; done; exit 0' >/dev/null 2>&1 || true
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

{
  echo "candidate_sha=$CANDIDATE_SHA"
  echo "worktree_dirty=$WORKTREE_DIRTY"
  echo "image=$IMAGE"
  uname -a
  grep -m1 'model name' /proc/cpuinfo || true
} > "$OUT/run.txt"

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/parout \
  "$IMAGE" \
  bash /court/suites/phase16/par-differential.sh 2>&1 | tee "$OUT/console.log"
