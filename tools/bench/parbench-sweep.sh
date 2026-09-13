#!/bin/sh
# §16.8.7 threshold sweep: best-of-N child parses per (shape,size,threshold).
# Emits CSV: shape,bytes,threshold,off_ns,auto_ns,speedup,mbps
set -eu
BIN=./target/release/examples/parbench
REPS="${REPS:-3}"
THREADS="${THREADS:-8}"
SHAPES="${SHAPES:-one_comment many_comment one_cdata many_cdata}"
SIZES="${SIZES:-262144 1048576 4194304 16777216 67108864}"
THRESHOLDS="${THRESHOLDS:-65536 131072 262144 1048576}"

best() { # shape size iters env...
  local shape="$1" size="$2" iters="$3"; shift 3
  local b="" r=0 v
  while [ "$r" -lt "$REPS" ]; do
    v=$(env "$@" "$BIN" --child "$shape" "$size" "$iters" | awk '{print $1}')
    if [ -z "$b" ] || [ "$(awk -v a="$v" -v c="$b" 'BEGIN{print (a<c)?1:0}')" = 1 ]; then b="$v"; fi
    r=$((r+1))
  done
  echo "$b"
}

echo "shape,bytes,threshold,off_ns,auto_ns,speedup,mbps"
for shape in $SHAPES; do
  for size in $SIZES; do
    off=$(best "$shape" "$size" 7 LIBXML_RS_PARALLEL=off LIBXML_RS_THREADS=1)
    for th in $THRESHOLDS; do
      auto=$(best "$shape" "$size" 7 LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS="$THREADS" LIBXML_RS_PARALLEL_THRESHOLD="$th")
      awk -v s="$shape" -v b="$size" -v t="$th" -v o="$off" -v a="$auto" \
        'BEGIN{printf "%s,%s,%s,%.0f,%.0f,%.3f,%.1f\n",s,b,t,o,a,o/a,(b/1048576)/(a/1e9)}'
    done
  done
done
