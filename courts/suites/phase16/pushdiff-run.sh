#!/bin/sh
# pushdiff-run.sh — §16.7.8 push-parser per-call differential court (host
# launcher). Runs pushdiff-differential.sh inside the oracle court VM over
# the adversarial push corpus + phase-14 fixtures and reports every
# (document, chunking-plan) cell whose oracle trace differs from the
# candidate trace, byte-for-byte.
#
# Evidentiary chain: the launcher refuses to run from anything but a
# pristine worktree (including untracked files — the courts tree is mounted
# into the container, so an untracked replacement court file must not be
# able to influence a run recorded as clean), and it BUILDS the candidate
# itself after establishing that clean source state, so the recorded
# candidate binary sha256 chains to the recorded candidate_sha through a
# deterministic build invocation:
#
#   clean committed tree
#     -> cargo clean -p libxml-rs --release
#     -> cargo build --locked --release --lib   (canonical target/release;
#        build.rs generates the lib//include/ layout into the first path
#        component named "target", so the build must NOT use a custom
#        --target-dir)
#     -> facade generation
#     -> candidate binary sha256
#     -> court run
#
# run.txt additionally records the Cargo.lock sha256, rustc version,
# probe/generator/runner fingerprints, and the oracle image ID + repo
# digest (a local tag like libxml-rs/phase14-debian:1 is mutable).
#
# Recommended workflow:
#   commit the court  ->  sh pushdiff-run.sh  ->  commit the evidence
#
# Env: SKIP_BUILD=1 reuses an existing candidate build (fast iteration;
# not the forensic default).
#
# Usage: sh courts/suites/phase16/pushdiff-run.sh [out-dir] [image]
# Env: BENCH_IMAGE default libxml-rs/phase14-debian:1.
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/pushdiff}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

# The clean-tree seal: NO modified/staged/untracked (non-ignored) files.
# candidate_sha / court_sha must name a committed state, and nothing
# outside git may influence the run.
if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all 2>/dev/null)" ]; then
  echo "pushdiff-run.sh: worktree is not pristine (modified, staged or" >&2
  echo "untracked files present) — commit everything first so the recorded" >&2
  echo "source shas and the clean-tree seal are unambiguous." >&2
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

# Build the candidate from the recorded source state. build.rs's
# find_target_dir walks up from OUT_DIR to the first component literally
# named "target", so cargo outputs and the generated lib//include/ layout
# can only share one directory: the canonical target/release. Cleaning the
# package first makes the binary-sha -> source-sha chain deterministic
# (the hashed .so cannot be a stale pre-existing build).
if [ "${SKIP_BUILD:-0}" != "1" ]; then
  echo "building candidate (target/release) ..."
  cargo clean --manifest-path "$ROOT/Cargo.toml" -p libxml-rs --release || exit 1
  cargo build --locked --manifest-path "$ROOT/Cargo.toml" \
    --release --lib || exit 1
  sh "$ROOT/tools/packaging/facade-gen.sh" "$ROOT/target/release" || exit 1
fi

CAND_DIR="$ROOT/target/release"
CAND_LIB="$(readlink -f "$CAND_DIR/lib/libxml2.so" 2>/dev/null || echo none)"
echo "candidate_sha=$CANDIDATE_SHA" > "$OUT/run.txt"
echo "court_sha=$CANDIDATE_SHA" >> "$OUT/run.txt"
echo "tree_clean=yes" >> "$OUT/run.txt"
echo "candidate_target_dir=$CAND_DIR" >> "$OUT/run.txt"
echo "candidate_libxml2_binary=$CAND_LIB" >> "$OUT/run.txt"
echo "candidate_libxml2_sha256=$(sha "$CAND_LIB")" >> "$OUT/run.txt"
echo "cargo_lock_sha256=$(sha "$ROOT/Cargo.lock")" >> "$OUT/run.txt"
rustc --version 2>/dev/null | sed 's/^/rustc=/;s/$/ (rustc --version at build time)/' >> "$OUT/run.txt" || true
cargo --version 2>/dev/null | sed 's/^/cargo=/' >> "$OUT/run.txt" || true
echo "probe_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-probe.c")" >> "$OUT/run.txt"
echo "generator_sha256=$(sha "$ROOT/courts/suites/phase16/gen_pushdiff_corpus.py")" >> "$OUT/run.txt"
echo "runner_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-differential.sh")" >> "$OUT/run.txt"
echo "probe_runner_sha256=$(sha "$ROOT/courts/suites/phase16/pushdiff-run.sh")" >> "$OUT/run.txt"
echo "image=$IMAGE" >> "$OUT/run.txt"
echo "pushdiff_filter=${PUSHDIFF_FILTER:-}" >> "$OUT/run.txt"
docker inspect -f '{{.Id}}' "$IMAGE" 2>/dev/null | sed 's/^/image_id=/' >> "$OUT/run.txt" || true
docker inspect -f '{{index .RepoDigests 0}}' "$IMAGE" 2>/dev/null | sed 's/^/image_digest=/' >> "$OUT/run.txt" || true
uname -a >> "$OUT/run.txt" 2>/dev/null || true
grep -m1 'model name' /proc/cpuinfo >> "$OUT/run.txt" || true

docker run --rm \
  -e PUSHDIFF_FILTER="${PUSHDIFF_FILTER:-}" \
  -v "$ROOT/courts":/court:ro \
  -v "$CAND_DIR":/candidate:ro \
  -v "$OUT":/scanout \
  "$IMAGE" \
  bash /court/suites/phase16/pushdiff-differential.sh 2>&1 | tee "$OUT/console.log"
