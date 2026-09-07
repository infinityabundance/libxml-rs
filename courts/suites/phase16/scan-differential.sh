#!/bin/bash
# scan-differential.sh — §16.7.7 parse-level differential, runs INSIDE the
# oracle court image (libxml-rs/phase14-debian:1).
#
# Compiles the node-dump probe against the ORACLE (/usr/local) and the
# CANDIDATE (/candidate), then parses every corpus document:
#
#   oracle : default probe
#   scalar / avx2 / avx512 : candidate probe with LIBXML_RS_SCAN_BACKEND
#                            forcing each §16.7 backend (env override is
#                            read once per process at first parse)
#
# Gates:
#   G1 — candidate backend invariance: scalar == avx2 == avx512 output,
#        byte-for-byte. THIS is the §16.7.7 gate: observable parser
#        behavior must not depend on the scanning backend.
#   G2 — oracle vs candidate-scalar diff (informational at this phase; the
#        pre-existing semantic divergences are tracked by the CLI courts).
#
# Corpus = adversarial generated docs (gen_scan_corpus.py) + the phase-14
# consumer fixture XMLs.
#
# Env: PROBE_SRC, FIXTURES, GEN, OUT (defaults below match the host
# launcher's mounts).
set -uo pipefail
export LC_ALL=C
GEN="${GEN:-/court/suites/phase16/gen_scan_corpus.py}"
PROBE_SRC="${PROBE_SRC:-/court/suites/phase14/consumers/node-dump-probe.c}"
FIXTURES="${FIXTURES:-/court/suites/phase14/sources}"
OUT="${OUT:-/scanout}"

mkdir -p "$OUT/corpus"
rm -f "$OUT/corpus"/*.xml
python3 "$GEN" "$OUT/corpus" || exit 1

# Assemble the file list (same absolute order for every run).
: > "$OUT/corpus.list"
find "$FIXTURES" -name '*.xml' -size -300k -print 2>/dev/null | sort >> "$OUT/corpus.list"
find "$OUT/corpus" -name '*.xml' -print | sort >> "$OUT/corpus.list"
nfiles=$(wc -l < "$OUT/corpus.list")
echo "corpus: $nfiles documents"

# Compile the probes.
cc -O2 -o "$OUT/probe-oracle" "$PROBE_SRC" \
  -I/usr/local/include/libxml2 -L/usr/local/lib -lxml2 -Wl,-rpath,/usr/local/lib || exit 1
cc -O2 -o "$OUT/probe-cand" "$PROBE_SRC" \
  -I/candidate/include/libxml2 -L/candidate/lib -lxml2 -Wl,-rpath,/candidate/lib || exit 1

mapfile -t FILES < "$OUT/corpus.list"

run_probe() { # tag binary [env]
  local tag="$1" bin="$2" envv="${3:-}"
  # stdout (the probe's own dumps) and stderr (libxml diagnostics) are kept
  # SEPARATE: the probe's stdout is block-buffered on a pipe while libxml's
  # stderr is unbuffered, so a merged stream interleaves differently between
  # providers — comparing merged streams would diff buffering, not behavior.
  if [ -n "$envv" ]; then
    env "$envv" "$bin" "${FILES[@]}" > "$OUT/out-$tag" 2> "$OUT/err-$tag"
  else
    "$bin" "${FILES[@]}" > "$OUT/out-$tag" 2> "$OUT/err-$tag"
  fi
  local rc=$?
  echo "probe $tag rc=$rc rows=$(wc -l < "$OUT/out-$tag") err=$(wc -l < "$OUT/err-$tag")"
  return $rc
}

run_probe oracle  "$OUT/probe-oracle" || exit 1
run_probe scalar  "$OUT/probe-cand" LIBXML_RS_SCAN_BACKEND=scalar || exit 1
run_probe avx2    "$OUT/probe-cand" LIBXML_RS_SCAN_BACKEND=avx2 || exit 1
run_probe avx512  "$OUT/probe-cand" LIBXML_RS_SCAN_BACKEND=avx512 || exit 1

# G1: candidate backend invariance — the §16.7.7 gate (stdout AND stderr
# must be byte-identical across backends).
fail=0
for pair in "scalar avx2" "scalar avx512"; do
  set -- $pair
  ok=1
  for f in out err; do
    if ! cmp -s "$OUT/$f-$1" "$OUT/$f-$2"; then
      echo "G1 FAIL backend-invariance: $f-$1 != $f-$2"
      diff "$OUT/$f-$1" "$OUT/$f-$2" | head -20
      ok=0
    fi
  done
  if [ $ok = 1 ]; then
    echo "G1 PASS backend-invariance: $1 == $2 (stdout+stderr)"
  else
    fail=1
  fi
done

# G2: oracle vs candidate (scalar reference backend), streams separated.
for f in out err; do
  if cmp -s "$OUT/$f-oracle" "$OUT/$f-scalar"; then
    echo "G2 PASS oracle == candidate(scalar): $f byte-identical"
  else
    echo "G2 DIFF oracle vs candidate(scalar): $f differs ($(diff "$OUT/$f-oracle" "$OUT/$f-scalar" | grep -c '^<') candidate-only lines)"
    diff "$OUT/$f-oracle" "$OUT/$f-scalar" | head -20
  fi
done

# Summary: hash each output for the receipt.
sha256sum "$OUT/out-"* "$OUT/err-"* 2>/dev/null | sed "s|$OUT/||" > "$OUT/sha256.txt"
cat "$OUT/sha256.txt"

exit $fail
