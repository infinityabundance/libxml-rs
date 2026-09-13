#!/bin/bash
# par-oracle-matrix.sh — §16.8.8 oracle-relative scaling matrix, runs INSIDE the
# oracle court image. Each provider loads in its own process (provider isolation,
# atlas §2): the oracle-linked harness and the candidate-linked harness are
# separate binaries, pinned with taskset and run in alternating order.
#
# Emits CSV: op,bytes,provider,threads,trial,wall_ns_per_iter,cpu_ns_per_iter
#
# Env: OUT (/out), CPUSET (0-15), TRIALS (7), SIZES.
set -eu
export LC_ALL=C
OUT="${OUT:-/out}"
HARNESS_SRC="${HARNESS_SRC:-/bench/harness.c}"
ORACLE=/usr/local
CAND=/candidate
CPUSET="${CPUSET:-0-15}"
TRIALS="${TRIALS:-7}"
SIZES="${SIZES:-1048576 4194304 16777216 67108864}"
OPS="${OPS:-parse_comment_many parse_comment_one parse_cdata_many parse_cdata_one parse_e2e}"

cc -O2 -o "$OUT/harness-oracle" "$HARNESS_SRC" \
  -I"$ORACLE/include/libxml2" -L"$ORACLE/lib" -lxml2 -lxslt -Wl,-rpath,"$ORACLE/lib"
cc -O2 -o "$OUT/harness-cand" "$HARNESS_SRC" \
  -I"$CAND/include/libxml2" -L"$CAND/lib" -lxml2 -lxslt -Wl,-rpath,"$CAND/lib"

echo "op,bytes,provider,threads,trial,wall_ns_per_iter,cpu_ns_per_iter" > "$OUT/par-oracle-matrix.csv"

cell() { # label bin env... -- op bytes iters
  local label="$1"; shift
  local bin="$1"; shift
  local envs=()
  while [ "$1" != "--" ]; do envs+=("$1"); shift; done
  shift
  local op="$1" bytes="$2" iters="$3"
  local w c
  if [ "${#envs[@]}" -gt 0 ]; then
    w=$(env "${envs[@]}" taskset -c "$CPUSET" "$bin" "$op" "$bytes" "$iters" 1 2 \
      | awk -F, '$1!="RSS" {printf "%s,%s\n", $5, $6}' | head -1)
  else
    w=$(taskset -c "$CPUSET" "$bin" "$op" "$bytes" "$iters" 1 2 \
      | awk -F, '$1!="RSS" {printf "%s,%s\n", $5, $6}' | head -1)
  fi
  echo "$label,$w"
}

for op in $OPS; do
  for bytes in $SIZES; do
    # Upstream rejects a single comment/CDATA run longer than
    # XML_MAX_TEXT_LENGTH (10 000 000): the "_one" shapes above 4 MiB would
    # measure the oracle's early bail-out, not a parse. The "_many" shapes keep
    # their runs at total/64, so every size is valid for them.
    case "$op" in
      *_one) if [ "$bytes" -gt 4194304 ]; then continue; fi;;
    esac
    # Calibrate iters so a trial is ~30 ms on the oracle.
    base=$(taskset -c "$CPUSET" "$OUT/harness-oracle" "$op" "$bytes" 1 1 1 \
      | awk -F, 'NR==1 {print $5}')
    iters=$(awk -v w="$base" 'BEGIN {n=int(30000000/w); if (n<1) n=1; if (n>5000000) n=5000000; print n}')
    echo "  $op @ $bytes: oracle single ${base} ns -> iters=$iters"
    for t in $(seq 1 "$TRIALS"); do
      if [ $((t % 2)) -eq 1 ]; then
        order="oracle cand-off cand-auto8 cand-auto16 cand-on8"
      else
        order="cand-on8 cand-auto16 cand-auto8 cand-off oracle"
      fi
      for cell_label in $order; do
        case "$cell_label" in
          oracle)      line=$(cell oracle "$OUT/harness-oracle" -- "$op" "$bytes" "$iters");;
          cand-off)    line=$(cell cand-off "$OUT/harness-cand" LIBXML_RS_PARALLEL=off -- "$op" "$bytes" "$iters");;
          cand-auto8)  line=$(cell cand-auto8 "$OUT/harness-cand" LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS=8 -- "$op" "$bytes" "$iters");;
          cand-auto16) line=$(cell cand-auto16 "$OUT/harness-cand" LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS=16 -- "$op" "$bytes" "$iters");;
          cand-on8)    line=$(cell cand-on8 "$OUT/harness-cand" LIBXML_RS_PARALLEL=on LIBXML_RS_THREADS=8 -- "$op" "$bytes" "$iters");;
        esac
        provider="${line%%,*}"; rest="${line#*,}"
        wall="${rest%%,*}"; cpu="${rest#*,}"
        threads=1
        case "$provider" in
          cand-auto8|cand-on8) threads=8;;
          cand-auto16) threads=16;;
        esac
        echo "$op,$bytes,$provider,$threads,$t,$wall,$cpu" >> "$OUT/par-oracle-matrix.csv"
      done
    done
  done
done
echo "matrix written: $OUT/par-oracle-matrix.csv"

# Derived summary (median wall ns per cell + oracle-relative speedup) so the
# compact evidence survives the next run's cleanup sweep.
python3 - "$OUT/par-oracle-matrix.csv" "$OUT/summary.json" <<'PY'
import csv, json, statistics, collections, sys
rows = list(csv.DictReader(open(sys.argv[1])))
g = collections.defaultdict(list)
for r in rows:
    g[(r["op"], int(r["bytes"]), r["provider"])].append(float(r["wall_ns_per_iter"]))
def med(op, b, p):
    v = g.get((op, b, p), [])
    return statistics.median(v) if v else None
out = []
for (op, b) in sorted({(o, bb) for (o, bb, _) in g}):
    o = med(op, b, "oracle")
    if o is None:
        continue
    row = {"op": op, "bytes": b, "oracle_ns": o}
    for p in ("cand-off", "cand-auto8", "cand-auto16", "cand-on8"):
        c = med(op, b, p)
        row[p + "_ns"] = c
        row["speedup_" + p] = (o / c) if c else None
    out.append(row)
json.dump(out, open(sys.argv[2], "w"), indent=1)
print("summary written:", sys.argv[2])
PY
