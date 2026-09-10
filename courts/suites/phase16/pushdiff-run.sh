#!/bin/sh
# pushdiff-run.sh — §16.7.8 push-parser per-call differential court (host
# launcher). Runs pushdiff-differential.sh inside the oracle court VM over
# the adversarial push corpus + phase-14 fixtures and reports every
# (document, chunking-plan) cell whose oracle trace differs from the
# candidate trace, byte-for-byte.
#
# Evidentiary chain: the launcher REFUSES to run from a dirty worktree, so
# the shas recorded in run.txt unambiguously identify the committed court
# source AND the committed candidate source that produced the result.
# run.txt additionally records cryptographic fingerprints of the probe /
# corpus generator / runner, the candidate library binary actually mounted
# as /candidate, and the oracle container image ID + repo digest (a local
# tag like libxml-rs/phase14-debian:1 is mutable; the ID/digest is not).
#
# Recommended workflow:
#   commit the court        →  sh pushdiff-run.sh   →  commit the evidence
#
# Usage: sh courts/suites/phase16/pushdiff-run.sh [out-dir] [image]
# Env: BENCH_IMAGE default libxml-rs/phase14-debian:1. Requires a release
# build: cargo build --release --lib && sh tools/packaging/facade-gen.sh
# target/release (so /candidate carries the current engine).
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/pushdiff}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

# Refuse a dirty tree: candidate_source_sha / court_source_sha must name a
# committed state, not a working tree.
if ! git -C "$ROOT" diff --quiet 2>/dev/null || \
   ! git -C "$ROOT" diff --cached --quiet 2>/dev/null; then
  echo "pushdiff-run.sh: working tree is dirty — commit the court first so" >&2
  echo "the recorded source shas are unambiguous." >&2
  exit 1
fi

# Files created by earlier docker runs are root-owned; clean through docker.
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  docker run --rm -v "$OUT":/scanout "$IMAGE" bash -lc 'rm -rf /scanout/* /scanout/.[!.]* 2>/dev/null; exit 0' >/dev/null 2>&1 || true
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

sha() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }

CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
CAND_LIB="$(readlink -f "$ROOT/target/release/lib/libxml2.so" 2>/dev/null || echo none)"
echo "candidate_sha=$CANDIDATE_SHA" > "$OUT/run.txt"
echo "court_sha=$CANDIDATE_SHA" >> "$OUT/run.txt"
echo "tree_clean=yes" >> "$OUT/run.txt"
echo "candidate_libxml2_binary=$CAND_LIB" >> "$OUT/run.txt"
echo "candidate_libxml2_sha256=$(sha "$CAND_LIB")" >> "$OUT/run.txt"
echo "probe_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-probe.c")" >> "$OUT/run.txt"
echo "generator_sha256=$(sha "$ROOT/courts/suites/phase16/gen_pushdiff_corpus.py")" >> "$OUT/run.txt"
echo "runner_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-differential.sh")" >> "$OUT/run.txt"
echo "probe_runner_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-run.sh")" >> "$OUT/run.txt"
echo "image=$IMAGE" >> "$OUT/run.txt"
docker inspect -f '{{.Id}}' "$IMAGE" 2>/dev/null | sed 's/^/image_id=/' >> "$OUT/run.txt" || true
docker inspect -f '{{index .RepoDigests 0}}' "$IMAGE" 2>/dev/null | sed 's/^/image_digest=/' >> "$OUT/run.txt" || true
uname -a >> "$OUT/run.txt" 2>/dev/null || true
grep -m1 'model name' /proc/cpuinfo >> "$OUT/run.txt" || true

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$ROOT/target/release":/candidate:ro \
  -v "$OUT":/scanout \
  "$IMAGE" \
  bash /court/suites/phase16/pushdiff-differential.sh 2>&1 | tee "$OUT/console.log"
