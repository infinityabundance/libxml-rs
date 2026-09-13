#!/bin/bash
# par-differential.sh — §16.8 Rayon parallel-blocking differential, runs INSIDE
# the oracle court image (libxml-rs/phase14-debian:1).
#
# Compiles the node-dump probe against the CANDIDATE (/candidate) and parses
# every corpus document under every scanner configuration:
#
#   ref    LIBXML_RS_PARALLEL=off            LIBXML_RS_SCAN_BACKEND=scalar
#   avx2   LIBXML_RS_PARALLEL=off            LIBXML_RS_SCAN_BACKEND=avx2
#   avx512 LIBXML_RS_PARALLEL=off            LIBXML_RS_SCAN_BACKEND=avx512
#   auto   LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS=1|4|8|16
#   forced LIBXML_RS_PARALLEL=on   LIBXML_RS_THREADS=1|4|8|16
#   thr*   the auto path with a tiny forcing threshold (index on nearly every
#          document — maximum parallel-path coverage)
#
# Gates (§16.8.4 "an incorrect parallel parse is failure"):
#   G1 — SIMD backend invariance at PARALLEL=off: scalar == avx2 == avx512.
#   G2 — parallel invariance: ref == every auto/forced/threshold cell.
#   G3 — the push/incremental path never builds the per-parse index, so a
#        second pass with LIBXML_RS_PARALLEL=on over the same files must also
#        match (covered by G2's forced cells; PUSH is exercised by the §16.7.8
#        pushdiff court).
#
# stdout (the probe's node dumps) and stderr (libxml diagnostics) are compared
# separately: the probe stdout is block-buffered on a pipe while libxml's
# stderr is unbuffered, so a merged stream would diff buffering, not behavior.
#
# Env: GEN, PROBE_SRC, FIXTURES, OUT.
set -uo pipefail
export LC_ALL=C
GEN="${GEN:-/court/suites/phase16/gen_par_corpus.py}"
PROBE_SRC="${PROBE_SRC:-/court/suites/phase14/consumers/node-dump-probe.c}"
FIXTURES="${FIXTURES:-/court/suites/phase14/sources}"
OUT="${OUT:-/parout}"

mkdir -p "$OUT/corpus"
rm -f "$OUT/corpus"/*.xml
python3 "$GEN" "$OUT/corpus" || exit 1

: > "$OUT/corpus.list"
find "$FIXTURES" -name '*.xml' -size -400k -print 2>/dev/null | sort >> "$OUT/corpus.list"
find "$OUT/corpus" -name '*.xml' -print | sort >> "$OUT/corpus.list"
nfiles=$(wc -l < "$OUT/corpus.list")
echo "corpus: $nfiles documents"

cc -O2 -o "$OUT/probe-cand" "$PROBE_SRC" \
  -I/candidate/include/libxml2 -L/candidate/lib -lxml2 -Wl,-rpath,/candidate/lib || exit 1

mapfile -t FILES < "$OUT/corpus.list"

run_probe() { # tag env...
  local tag="$1"; shift
  env "$@" "$OUT/probe-cand" "${FILES[@]}" > "$OUT/out-$tag" 2> "$OUT/err-$tag"
  local rc=$?
  echo "probe $tag rc=$rc rows=$(wc -l < "$OUT/out-$tag") err=$(wc -l < "$OUT/err-$tag")"
  return 0
}

run_probe ref    LIBXML_RS_PARALLEL=off LIBXML_RS_SCAN_BACKEND=scalar
run_probe avx2   LIBXML_RS_PARALLEL=off LIBXML_RS_SCAN_BACKEND=avx2
run_probe avx512 LIBXML_RS_PARALLEL=off LIBXML_RS_SCAN_BACKEND=avx512
for t in 1 2 4 8 16; do
  run_probe "auto_t$t" LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS="$t"
  run_probe "on_t$t"   LIBXML_RS_PARALLEL=on   LIBXML_RS_THREADS="$t"
done
# Force the whole-input prepass on every document (threshold 1 byte) at the
# production thread counts; this is the strongest parallel-path coverage.
for t in 2 8 16; do
  run_probe "auto_thr0_t$t" LIBXML_RS_PARALLEL=auto LIBXML_RS_THREADS="$t" \
    LIBXML_RS_PARALLEL_THRESHOLD=1
done

compare() { # label a b
  local label="$1" a="$2" b="$3" ok=1 f
  for f in out err; do
    if ! cmp -s "$OUT/$f-$a" "$OUT/$f-$b"; then
      echo "FAIL $label: $f-$a != $f-$b"
      diff "$OUT/$f-$a" "$OUT/$f-$b" | head -12
      ok=0
    fi
  done
  if [ $ok = 1 ]; then echo "PASS $label: $a == $b (stdout+stderr)"; fi
  return $((1 - ok))
}

fail=0
# G1 — SIMD backend invariance (the §16.7.7 gate, now pinned to PARALLEL=off so
# the SIMD comparison is isolated from the Rayon path).
for be in avx2 avx512; do
  compare "G1 backend-invariance ref==$be" ref "$be" || fail=1
done
# G2 — every parallel configuration is byte-identical to the sequential ref.
for tag in auto_t1 auto_t2 auto_t4 auto_t8 auto_t16 \
           on_t1 on_t2 on_t4 on_t8 on_t16 \
           auto_thr0_t2 auto_thr0_t8 auto_thr0_t16; do
  compare "G2 parallel-invariance ref==$tag" ref "$tag" || fail=1
done

sha256sum "$OUT/out-"* "$OUT/err-"* 2>/dev/null | sed "s|$OUT/||" > "$OUT/sha256.txt"
cat "$OUT/sha256.txt"
exit $fail
