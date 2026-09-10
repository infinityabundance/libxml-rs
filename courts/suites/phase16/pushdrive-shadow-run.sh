#!/bin/sh
# pushdrive-shadow-run.sh — §16.7.8 pushdrive ORACLE-SHADOW court (host
# launcher).
#
# Runs the pushdiff probe against the SYSTEM libxml2 (the oracle) only, over
# the persistent driver's shadow corpus and a fixed plan list, and commits the
# resulting per-call traces as fixtures. The companion Rust court
# (src/xml/parser/pushshadow.rs) drives the persistent DRIVER over the same
# documents/plans and compares its trace against these fixtures.
#
# Why the oracle side is generated here rather than in Rust: the oracle is a C
# library reachable only through the probe binary, while the driver is
# court-only Rust that `xmlParseChunk` does not (yet) call. The two sides are
# therefore produced by different programs, which is exactly why the corpus is
# GENERATED (gen_shadow_corpus.py) and the plan list is fixed and recorded.
#
# Evidentiary chain: the launcher refuses to run from anything but a pristine
# worktree, and records the source sha, the probe/generator fingerprints, the
# oracle image ID and the host description in manifest.txt next to the traces.
#
# Usage: sh courts/suites/phase16/pushdrive-shadow-run.sh [out-dir] [image]
# Env: SHADOW_IMAGE default libxml-rs/phase14-debian:1.
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/pushdrive-shadow}"
IMAGE="${SHADOW_IMAGE:-libxml-rs/phase14-debian:1}"

if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all 2>/dev/null)" ]; then
  echo "pushdrive-shadow-run.sh: worktree is not pristine (modified, staged or" >&2
  echo "untracked files present) — commit everything first so the recorded" >&2
  echo "source shas and the clean-tree seal are unambiguous." >&2
  exit 1
fi

# Files created by earlier docker runs are root-owned; clean through docker.
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  docker run --rm -v "$OUT":/scanout "$IMAGE" bash -lc 'cd /scanout; for f in * .[!.]*; do [ "$f" = ".gitignore" ] || rm -rf -- "$f"; done; exit 0' >/dev/null 2>&1 || true
fi
mkdir -p "$OUT"

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$OUT":/scanout \
  "$IMAGE" bash -lc '
set -euo pipefail
export LC_ALL=C
mkdir -p /tmp/shadow
# The corpus is COMMITTED (courts/suites/phase16/shadow-corpus) so the oracle
# probe and the Rust driver court read byte-identical documents. The generator
# is run first only to prove it still reproduces them exactly.
python3 /court/suites/phase16/gen_shadow_corpus.py /tmp/shadow/gen > /dev/null
for f in /court/suites/phase16/shadow-corpus/*.xml; do
  base=$(basename "$f")
  if ! cmp -s "$f" "/tmp/shadow/gen/$base"; then
    echo "shadow corpus drift: $base does not match gen_shadow_corpus.py" >&2
    exit 1
  fi
done
cc -O1 -Wall -Wextra -Werror -o /tmp/shadow/probe \
  /court/suites/phase16/pushdiff-probe.c \
  -I/usr/local/include/libxml2 -L/usr/local/lib -lxml2 -Wl,-rpath,/usr/local/lib
for f in /court/suites/phase16/shadow-corpus/*.xml; do
  base=$(basename "$f")
  sz=$(stat -c %s "$f")
  case "$base" in
    shadow-win-*) modes="b1024 b4096 b1024i" ;;
    *)
      if [ "$sz" -le 200 ]; then
        modes="b1 b2 b3 b5 b257 Cb1 b1z2 b1i Cb1i r9-2"
      else
        modes="b1024 b4096 b1024i r17-512"
      fi ;;
  esac
  for m in $modes; do
    /tmp/shadow/probe -W "$m" "$f" > "/scanout/oracle-${base}__${m}"
  done
done
echo "oracle traces: $(ls -1 /scanout | wc -l)"
'

{
  # This manifest describes ORACLE-ONLY fixture generation. It deliberately does
  # not use a `candidate_sha` field: there is no candidate in this run. The
  # driver-side gate that CONSUMES these fixtures names its own commit
  # separately (see the receipt that references this directory).
  echo "side=oracle-only (system libxml2)"
  echo "fixture_generation_sha=$(git -C "$ROOT" rev-parse HEAD)"
  echo "fixture_tree_clean=yes"
  echo "fixture_consumer_note=the Rust gate src/xml/parser/pushshadow.rs consumes these; it pins the expected plan matrix independently"
  echo "corpus=courts/suites/phase16/shadow-corpus (verified against gen_shadow_corpus.py in-run)"
  echo "probe_sha256=$(sha256sum "$ROOT/courts/suites/phase16/pushdiff-probe.c" | cut -d" " -f1)"
  echo "generator_sha256=$(sha256sum "$ROOT/courts/suites/phase16/gen_shadow_corpus.py" | cut -d" " -f1)"
  echo "runner_sha256=$(sha256sum "$ROOT/courts/suites/phase16/pushdrive-shadow-run.sh" | cut -d" " -f1)"
  echo "plans_small=b1,b2,b3,b5,b257,Cb1,b1z2,b1i,Cb1i,r9-2"
  echo "plans_long=b1024,b4096,b1024i,r17-512"
  echo "plans_window=b1024,b4096,b1024i (shadow-win-* documents)"
  echo "probe_flags=-W (physical-window mode: emits consumed/abs)"
  echo "image=$IMAGE"
  echo "image_id=$(docker image inspect --format "{{.Id}}" "$IMAGE" 2>/dev/null || echo unknown)"
  uname -a
} > "$OUT/manifest.txt"

echo "wrote $OUT"
ls -1 "$OUT" | head -8
echo "..."
echo "total files: $(ls -1 "$OUT" | wc -l)"
