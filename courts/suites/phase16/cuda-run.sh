#!/bin/sh
# cuda-run.sh — §16.9.6 GPU-vs-CPU differential launcher.
#
# Builds a CUDA-ENABLED candidate into the canonical target dir (build.rs only
# emits the `lib/`+`include/` consumer layout there), runs the court, then
# restores the default CPU-only artefact so later courts are unaffected.
#
# The court runs on the HOST: the GPU device and `libcuda.so.1` live on the
# host, and this court compares the candidate against *itself* under
# `LIBXML_RS_ACCEL=cpu|cuda` — no oracle is involved, so provider isolation is
# trivially satisfied (one provider per probe process).
#
# Usage: sh courts/suites/phase16/cuda-run.sh [out-dir]
# Env: KEEP_CUDA_BUILD=1 leaves the CUDA-enabled DSO in place.
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/16-9-cuda}"
CAND="$ROOT/target/release"

CANDIDATE_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
WORKTREE_DIRTY="$( [ -n "$(git -C "$ROOT" status --porcelain -- . ':(exclude)courts/receipts/phase-16/raw' 2>/dev/null)" ] && echo yes || echo no )"

echo "building cuda-enabled candidate DSO (target/release --features cuda)..."
cargo build --manifest-path "$ROOT/Cargo.toml" --release --lib --features cuda >/dev/null
sh "$ROOT/tools/packaging/facade-gen.sh" "$CAND"

if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  for f in "$OUT"/out-* "$OUT"/err-* "$OUT"/probe-* "$OUT"/corpus "$OUT"/precondition.err; do
    [ -e "$f" ] && rm -rf -- "$f"
  done
fi
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"

{
  echo "candidate_sha=$CANDIDATE_SHA"
  echo "worktree_dirty=$WORKTREE_DIRTY"
  echo "features=cuda"
  uname -a
  grep -m1 'model name' /proc/cpuinfo || true
  (nvidia-smi --query-gpu=name,compute_cap,driver_version,pcie.link.gen.max,pcie.link.width.max \
    --format=csv,noheader 2>/dev/null || echo "no nvidia-smi") | sed 's/^/gpu: /'
} > "$OUT/run.txt"

set +e
env \
  GEN="$ROOT/courts/suites/phase16/gen_par_corpus.py" \
  PROBE_SRC="$ROOT/courts/suites/phase14/consumers/node-dump-probe.c" \
  FIXTURES="$ROOT/courts/suites/phase14/sources" \
  CAND_DIR="$CAND" \
  OUT="$OUT" \
  bash "$ROOT/courts/suites/phase16/cuda-differential.sh" 2>&1 | tee "$OUT/console.log"
rc=$?
set -e

# Restore the default CPU-only artefact for subsequent courts.
if [ "${KEEP_CUDA_BUILD:-0}" != "1" ]; then
  echo "restoring default (CPU-only) candidate DSO..."
  cargo build --manifest-path "$ROOT/Cargo.toml" --release --lib >/dev/null
  sh "$ROOT/tools/packaging/facade-gen.sh" "$CAND"
fi
exit $rc
