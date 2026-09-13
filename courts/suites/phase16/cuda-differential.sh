#!/bin/bash
# cuda-differential.sh — §16.9.6 GPU-vs-CPU parse differential, runs INSIDE the
# oracle court image against a candidate DSO built with `--features cuda`.
#
# The GPU Stage-1 classifier must be observably identical to the CPU scanner:
# the same blocks are fed to the same CPU Stage-2, so every parse tree and every
# diagnostic must match across `LIBXML_RS_ACCEL=cpu|cuda`, with the prepass
# forced on (`LIBXML_RS_PARALLEL_THRESHOLD=1`) so it fires for every document.
#
# Precondition (fails the court if the DSO lacks the feature): a debug run under
# `LIBXML_RS_ACCEL=cuda` must report `[cuda] initialized`. Without it the
# comparison would be vacuously true.
#
# Corpus: the §16.8 boundary corpus + phase-14 fixtures + explicit
# DOCTYPE/internal-subset/entity/Unicode documents (§16.9.6).
#
# Env: GEN, PROBE_SRC, FIXTURES, OUT.
set -uo pipefail
export LC_ALL=C
GEN="${GEN:-/court/suites/phase16/gen_par_corpus.py}"
PROBE_SRC="${PROBE_SRC:-/court/suites/phase14/consumers/node-dump-probe.c}"
FIXTURES="${FIXTURES:-/court/suites/phase14/sources}"
OUT="${OUT:-/cudaout}"
# Candidate DSO directory (container: /candidate; host launcher overrides).
CAND_DIR="${CAND_DIR:-/candidate}"

mkdir -p "$OUT/corpus"
rm -f "$OUT/corpus"/*.xml
python3 "$GEN" "$OUT/corpus" || exit 1

# §16.9.6 explicit constructs: DOCTYPE/internal subset, entities, Unicode,
# malformed declarations, long attributes, comments/CDATA crossings.
python3 - "$OUT/corpus" <<'PY'
import os, sys
d = sys.argv[1]
docs = {
 "z_internal_subset.xml": b'<!DOCTYPE r [<!ELEMENT r (#PCDATA|b)*><!ELEMENT b EMPTY><!ENTITY e "<b/>">]><r>x&e;y</r>',
 "z_entity_value.xml": b'<!DOCTYPE r [<!ENTITY c "<!-- comment -->"><!ENTITY t "[CDATA-like ]]> text">]><r>&c;&t;</r>',
 "z_unicode.xml": "<r a=\"\u00e9\">\u4e2d\u6587 \u20ac \U0001F600 tail</r>".encode(),
 "z_malformed_decl.xml": b'<?xml\t\xf0',
 "z_long_attr.xml": b'<r a="' + b'-]?<&' * 5000 + b'">t</r>',
 "z_cdata_comment_cross.xml": b'<r><![CDATA[' + b']' * 5000 + b']]><!--' + b'-' * 5000 + b'--></r>',
 "z_attrs.xml": b'<r ' + b' '.join(b'a%d="v%d"' % (i, i) for i in range(2000)) + b'/>',
}
for name, data in docs.items():
    open(os.path.join(d, name), "wb").write(data)
print(f"wrote {len(docs)} constructs")
PY

: > "$OUT/corpus.list"
find "$FIXTURES" -name '*.xml' -size -400k -print 2>/dev/null | sort >> "$OUT/corpus.list"
find "$OUT/corpus" -name '*.xml' -print | sort >> "$OUT/corpus.list"
nfiles=$(wc -l < "$OUT/corpus.list")
echo "corpus: $nfiles documents"

cc -O2 -o "$OUT/probe-cand" "$PROBE_SRC" \
  -I"$CAND_DIR/include/libxml2" -L"$CAND_DIR/lib" -lxml2 -Wl,-rpath,"$CAND_DIR/lib" || exit 1

mapfile -t FILES < "$OUT/corpus.list"

# ── Precondition: the candidate DSO really has the CUDA path ────────────────
# Pick a document whose comment run exceeds the probe floor (BLOCK = 64 KiB) so
# the prepass (and therefore the GPU) actually fires.
precondition_doc=$(grep 'c_one_' "$OUT/corpus.list" | sort | tail -1)
if [ -z "$precondition_doc" ]; then precondition_doc="${FILES[0]}"; fi
env LIBXML_RS_ACCEL=cuda LIBXML_RS_ACCEL_DEBUG=1 LIBXML_RS_PARALLEL_THRESHOLD=1 \
  "$OUT/probe-cand" "$precondition_doc" >/dev/null 2> "$OUT/precondition.err"
if ! grep -q '\[cuda\] initialized' "$OUT/precondition.err"; then
  echo "PRECONDITION FAIL: candidate DSO did not initialize CUDA"
  echo "(build the candidate with --features cuda)"
  cat "$OUT/precondition.err"
  exit 1
fi
echo "PRECONDITION PASS: CUDA device initialized"

run_probe() { # tag env...
  local tag="$1"; shift
  env "$@" "$OUT/probe-cand" "${FILES[@]}" > "$OUT/out-$tag" 2> "$OUT/err-$tag"
  local rc=$?
  echo "probe $tag rc=$rc rows=$(wc -l < "$OUT/out-$tag") err=$(wc -l < "$OUT/err-$tag")"
  return 0
}

run_probe ref       LIBXML_RS_ACCEL=cpu LIBXML_RS_PARALLEL=off LIBXML_RS_SCAN_BACKEND=scalar
run_probe cpu_auto  LIBXML_RS_ACCEL=cpu LIBXML_RS_PARALLEL=auto
run_probe cpu_thr0  LIBXML_RS_ACCEL=cpu LIBXML_RS_PARALLEL=auto LIBXML_RS_PARALLEL_THRESHOLD=1
for t in 1 4 8; do
  run_probe "cuda_t$t" LIBXML_RS_ACCEL=cuda LIBXML_RS_THREADS="$t"
done
run_probe cuda_thr0 LIBXML_RS_ACCEL=cuda LIBXML_RS_PARALLEL_THRESHOLD=1
run_probe cuda_cpu_index LIBXML_RS_ACCEL=cuda LIBXML_RS_PARALLEL=off

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
# G1 — every parse under a CUDA-selected index is byte-identical to the
# sequential CPU reference (trees AND diagnostics).
for tag in cpu_auto cpu_thr0 cuda_t1 cuda_t4 cuda_t8 cuda_thr0 cuda_cpu_index; do
  compare "G1 cuda-invariance ref==$tag" ref "$tag" || fail=1
done
# G2 — the GPU cells are byte-identical to the CPU-parallel cells.
compare "G2 cuda==cpu_auto" cuda_t4 cpu_auto || fail=1
compare "G2 cuda==cpu_thr0" cuda_thr0 cpu_thr0 || fail=1

sha256sum "$OUT/out-"* "$OUT/err-"* 2>/dev/null | sed "s|$OUT/||" > "$OUT/sha256.txt"
cat "$OUT/sha256.txt"
exit $fail
