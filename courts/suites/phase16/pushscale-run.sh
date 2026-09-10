#!/bin/sh
# pushscale-run.sh — §16.7.8 O(N) scaling court (host launcher).
#
# Runs courts/suites/phase16/pushscale.c against BOTH providers over two
# shapes and records the wall-clock curve:
#
#   small-chunk  ~1 KiB chunks (20k calls for 20 MB) — the shape that exposed
#                the replay engine's O(N^2): every xmlParseChunk re-parsed the
#                whole accumulated buffer, so the old curve was
#                6.4 s -> 24.6 s -> 96.0 s for 20/40/80 MB.
#   large-chunk  1 MiB chunks, ~5/10/21 GB total — proves the complexity class
#                over two doublings at a size no quadratic engine can reach.
#
# The document is newline-heavy text plus a <br/> element per chunk, i.e. the
# lxml test_very_large_sourceline_iterparse shape. The push context is created
# with a NULL SAX handler (the default tree builder) and default options, which
# is exactly the configuration the persistent driver is eligible for.
#
# Evidentiary chain: refuses to run from anything but a pristine worktree,
# builds the candidate itself after establishing that state, and records the
# source sha, the candidate .so sha256, the probe hash and the oracle image.
#
# Usage: sh courts/suites/phase16/pushscale-run.sh [out-dir] [image]
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${1:-$ROOT/courts/receipts/phase-16/raw/pushscale}"
IMAGE="${BENCH_IMAGE:-libxml-rs/phase14-debian:1}"

if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=all 2>/dev/null)" ]; then
  echo "pushscale-run.sh: worktree is not pristine — commit everything first." >&2
  exit 1
fi

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
sha() { sha256sum "$1" 2>/dev/null | cut -d' ' -f1; }

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  echo "building candidate (target/release) ..."
  cargo clean --manifest-path "$ROOT/Cargo.toml" -p libxml-rs --release || exit 1
  cargo build --locked --manifest-path "$ROOT/Cargo.toml" --release --lib || exit 1
  sh "$ROOT/tools/packaging/facade-gen.sh" "$ROOT/target/release" || exit 1
fi
CAND_DIR="$ROOT/target/release"
CAND_LIB="$(readlink -f "$CAND_DIR/lib/libxml2.so" 2>/dev/null || echo none)"

{
  echo "candidate_sha=$(git -C "$ROOT" rev-parse HEAD)"
  echo "tree_clean=yes"
  echo "candidate_libxml2_sha256=$(sha "$CAND_LIB")"
  echo "bench_sha256=$(sha "$ROOT/courts/suites/phase16/pushscale.c")"
  echo "runner_sha256=$(sha "$ROOT/courts/suites/phase16/pushscale-run.sh")"
  echo "image=$IMAGE"
  echo "image_id=$(docker image inspect --format '{{.Id}}' "$IMAGE" 2>/dev/null || echo unknown)"
  uname -a
  grep -m1 'model name' /proc/cpuinfo
} > "$OUT/run.txt"

docker run --rm \
  -v "$ROOT/courts":/court:ro \
  -v "$CAND_DIR":/candidate:ro \
  "$IMAGE" bash -lc '
set -euo pipefail
export LC_ALL=C
cc -O2 -o /tmp/ps-o /court/suites/phase16/pushscale.c \
  -I/usr/local/include/libxml2 -L/usr/local/lib -lxml2 -Wl,-rpath,/usr/local/lib
cc -O2 -o /tmp/ps-c /court/suites/phase16/pushscale.c \
  -I/candidate/include/libxml2 -L/candidate/lib -lxml2 -Wl,-rpath,/candidate/lib

echo "# small-chunk curve (1 KiB chunks): 20/40/80 MB"
for n in 20480 40960 81920; do
  echo -n "oracle    n=$n "; /tmp/ps-o "$n" 1
  echo -n "candidate n=$n "; /tmp/ps-c "$n" 1
done

echo "# large-chunk curve (1 MiB chunks): ~5/10/21 GB"
for n in 5120 10240 20480; do
  echo -n "oracle    n=$n "; /tmp/ps-o "$n" 1024
  echo -n "candidate n=$n "; /tmp/ps-c "$n" 1024
done
' | tee "$OUT/console.log"

echo "wrote $OUT/run.txt and $OUT/console.log"
