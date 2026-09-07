#!/bin/sh
# scan-backend-matrix.sh — §16.7.5/16.8-baseline: parse_e2e crossover of the
# candidate's scalar/avx2/avx512 scan backends vs the oracle, in the oracle
# court VM. Each provider+backend runs as its own process (the backend env
# override is read once per process), pinned with taskset, alternating
# order A/B/B/A. Emits a CSV of wall ns/iter per cell.
#
# Usage: run inside the court image with /bench (tools/bench) and
# /candidate (target/release facades) mounted; output to /out.
set -eu
export LC_ALL=C
OUT="${OUT:-/out}"
HARNESS_SRC=/bench/harness.c
ORACLE=/usr/local
CAND=/candidate
CPUSET="${CPUSET:-0}"
TRIALS="${TRIALS:-7}"
MIN_NS=30000000

cc -O2 -o "$OUT/harness-oracle" "$HARNESS_SRC" \
  -I"$ORACLE/include/libxml2" -L"$ORACLE/lib" -lxml2 -lxslt \
  -Wl,-rpath,"$ORACLE/lib"
cc -O2 -o "$OUT/harness-cand" "$HARNESS_SRC" \
  -I"$CAND/include/libxml2" -L"$CAND/lib" -lxml2 -lxslt \
  -Wl,-rpath,"$CAND/lib"

run_cell() { # bin env op bytes
  local bin="$1" envv="$2" op="$3" bytes="$4"
  local wall=""
  if [ -n "$envv" ]; then
    wall=$(env "$envv" taskset -c "$CPUSET" "$bin" "$op" "$bytes" 1 1 3 \
      | awk -F, 'NR==1 && $1!="RSS" {print $5}')
  else
    wall=$(taskset -c "$CPUSET" "$bin" "$op" "$bytes" 1 1 3 \
      | awk -F, 'NR==1 && $1!="RSS" {print $5}')
  fi
  echo "$wall"
}

echo "op,bytes,provider,backend,trial,wall_ns_per_iter" > "$OUT/scan-backend-matrix.csv"
for bytes in 1024 16384 131072 1048576 8388608; do
  for op in parse_e2e parse_ctx_reuse; do
    # calibrate iters on the oracle (single run) to ~MIN_NS
    iters=1000
    wall=$(run_cell "$OUT/harness-oracle" "" "$op" "$bytes")
    iters=$(awk -v w="$wall" -v m="$MIN_NS" 'BEGIN {n=int(m/w); if (n<10) n=10; if (n>20000000) n=20000000; print n}')
    echo "  $op @ $bytes bytes: iters=$iters (oracle single $wall ns)"
    # cells: oracle + candidate under each backend; alternating order
    for t in $(seq 1 "$TRIALS"); do
      if [ $((t % 2)) -eq 1 ]; then order="oracle scalar avx2 avx512"; else order="avx512 avx2 scalar oracle"; fi
      for cell in $order; do
        case "$cell" in
          oracle) w=$(taskset -c "$CPUSET" "$OUT/harness-oracle" "$op" "$bytes" "$iters" 1 3 | awk -F, 'NR==1 && $1!="RSS" {print $5}');;
          scalar) w=$(env LIBXML_RS_SCAN_BACKEND=scalar taskset -c "$CPUSET" "$OUT/harness-cand" "$op" "$bytes" "$iters" 1 3 | awk -F, 'NR==1 && $1!="RSS" {print $5}');;
          avx2)   w=$(env LIBXML_RS_SCAN_BACKEND=avx2 taskset -c "$CPUSET" "$OUT/harness-cand" "$op" "$bytes" "$iters" 1 3 | awk -F, 'NR==1 && $1!="RSS" {print $5}');;
          avx512) w=$(env LIBXML_RS_SCAN_BACKEND=avx512 taskset -c "$CPUSET" "$OUT/harness-cand" "$op" "$bytes" "$iters" 1 3 | awk -F, 'NR==1 && $1!="RSS" {print $5}');;
        esac
        echo "$op,$bytes,$cell,$t,$w" >> "$OUT/scan-backend-matrix.csv"
      done
    done
  done
done
echo "matrix written: $OUT/scan-backend-matrix.csv"
