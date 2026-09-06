#!/bin/bash
# profile.sh — Phase 16.4 profiling discipline (valgrind-backed, since `perf`
# is not installed on this host).
#
# Produces before/after profiles for a single harness operation and provider.
# Valgrind callgrind gives the call graph + instruction counts; cachegrind gives
# cache/branch statistics; massif gives heap allocation profiles — the three
# causality signals §16.4 requires when wall-clock alone is ambiguous.
#
# Usage:
#   profile.sh <prefix> <op> <bytes> <out_dir>
#
#   <prefix>    oracle prefix (/usr/local) or candidate prefix (/candidate)
#   <op>        a harness operation (e.g. parse_e2e, xpath_eval_compiled)
#   <bytes>     input size target
#   <out_dir>   where to write profile outputs
#
# Env: HARNESS_SRC (default tools/bench/harness.c), BENCH_CPUSET

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="${HARNESS_SRC:-$HERE/harness.c}"
PREFIX="${1:?usage: profile.sh <prefix> <op> <bytes> <out_dir>}"
OP="${2:?usage}"
BYTES="${3:?usage}"
OUT="${4:?usage}"
mkdir -p "$OUT"

INCLUDES=()
[ -d "$PREFIX/include/libxml2" ] && INCLUDES+=("-I$PREFIX/include/libxml2")
[ -d "$PREFIX/include" ] && INCLUDES+=("-I$PREFIX/include")

BIN="$OUT/harness"
cc -O2 -g0 "${INCLUDES[@]}" -L"$PREFIX/lib" "$SRC" -o "$BIN" \
  -lxml2 -lxslt "-Wl,-rpath,$PREFIX/lib"

RUN=("$BIN" "$OP" "$BYTES" "1" "1" "0")
if [ -n "${BENCH_CPUSET:-}" ] && command -v taskset >/dev/null 2>&1; then
  RUN=(taskset -c "$BENCH_CPUSET" "${RUN[@]}")
fi

# callgrind: call graph + instruction counts.
valgrind --tool=callgrind --callgrind-out-file="$OUT/callgrind.out" \
  --collect-atstart=yes "${RUN[@]}" >/dev/null 2>"$OUT/callgrind.err"
callgrind_annotate --inclusive=yes --threshold=1 "$OUT/callgrind.out" \
  >"$OUT/callgrind.annot" 2>/dev/null || true
callgrind_annotate --inclusive=no --threshold=1 "$OUT/callgrind.out" \
  >"$OUT/callgrind.self.annot" 2>/dev/null || true

# cachegrind: cache + branch statistics.
valgrind --tool=cachegrind --cachegrind-out-file="$OUT/cachegrind.out" \
  "${RUN[@]}" >/dev/null 2>"$OUT/cachegrind.err"
cg_annotate "$OUT/cachegrind.out" >"$OUT/cachegrind.annot" 2>/dev/null || true

# massif: heap allocation profile.
valgrind --tool=massif --massif-out-file="$OUT/massif.out" \
  --time-unit=B "${RUN[@]}" >/dev/null 2>"$OUT/massif.err"
ms_print "$OUT/massif.out" >"$OUT/massif.txt" 2>/dev/null || true

echo "profiles written to $OUT: callgrind.annot cachegrind.annot massif.txt"
