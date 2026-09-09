#!/bin/bash
# pushdiff-differential.sh — §16.7.8 push-parser per-call differential, runs
# INSIDE the oracle court image (libxml-rs/phase14-debian:1).
#
# Compiles pushdiff-probe.c against the ORACLE (/usr/local) and the
# CANDIDATE (/candidate), then feeds every corpus document to
# xmlCreatePushParserCtxt + xmlParseChunk under fixed and random chunking
# plans. The probe prints ONE merged, deterministic trace per (file, mode):
# per-call rc/errNo/wellFormed/instate interleaved with every SAX event and
# structured error record in invocation order.
#
# Gate G — oracle trace == candidate trace, byte-for-byte, for every
# (document, chunking plan, sax mode). This is the ground-truth gate the
# incremental push parser (16.7.8) must keep green: any divergence in
# event segmentation, arguments, per-call return codes, or diagnostics is a
# byte diff here.
#
# Env: GEN, PROBE_SRC, FIXTURES, OUT (defaults match the host launcher).
set -uo pipefail
export LC_ALL=C
GEN="${GEN:-/court/suites/phase16/gen_pushdiff_corpus.py}"
PROBE_SRC="${PROBE_SRC:-/court/suites/phase16/pushdiff-probe.c}"
FIXTURES="${FIXTURES:-/court/suites/phase14/sources}"
OUT="${OUT:-/scanout}"

mkdir -p "$OUT/corpus" "$OUT/diffs"
rm -f "$OUT/corpus"/*.xml
python3 "$GEN" "$OUT/corpus" || exit 1

# Document list (deterministic absolute order). Generated corpus first,
# then small phase-14 fixture XMLs (<= 32 KiB: the push court must stay
# affordable in both providers; big-doc push is covered by pushscale.c).
: > "$OUT/docs.list"
find "$OUT/corpus" -name '*.xml' -print | sort >> "$OUT/docs.list"
find "$FIXTURES" -name '*.xml' -size -32k -print 2>/dev/null | sort >> "$OUT/docs.list"
mapfile -t DOCS < "$OUT/docs.list"
echo "pushdiff docs: ${#DOCS[@]}"

# Compile the probe against both providers.
cc -O1 -w -o "$OUT/probe-oracle" "$PROBE_SRC" \
  -I/usr/local/include/libxml2 -L/usr/local/lib -lxml2 -Wl,-rpath,/usr/local/lib || exit 1
cc -O1 -w -o "$OUT/probe-cand" "$PROBE_SRC" \
  -I/candidate/include/libxml2 -L/candidate/lib -lxml2 -Wl,-rpath,/candidate/lib || exit 1

# Pick chunking plans by document size so single-byte chunking stays
# affordable while every byte offset is still covered across the corpus.
pick_modes() { # path -> echoes the sax2 modes to run
  local sz
  sz=$(stat -c %s "$1")
  if   [ "$sz" -le 200 ];  then echo "b1 b2 b3 b5 b7 b16 b257 r3 r9 r77"
  elif [ "$sz" -le 2000 ]; then echo "b1 b2 b5 b64 b257 r1 r42"
  elif [ "$sz" -le 9000 ]; then echo "b1 b7 b64 b1024 r2 r17"
  elif [ "$sz" -le 65536 ]; then echo "b16 b4096 b32768 r5 r23"
  else echo "b4096 b65536 r11"
  fi
}

fail=0
total=0
: > "$OUT/summary.txt"

for doc in "${DOCS[@]}"; do
  rel=${doc#$OUT/}
  modes=$(pick_modes "$doc")
  for m in $modes; do
    total=$((total + 1))
    tag=$(echo "$rel" | tr '/' '_')__$m
    "$OUT/probe-oracle" "$m" "$doc" > "$OUT/oracle-$tag" 2>&1
    "$OUT/probe-cand"   "$m" "$doc" > "$OUT/cand-$tag"   2>&1
    if ! cmp -s "$OUT/oracle-$tag" "$OUT/cand-$tag"; then
      fail=$((fail + 1))
      echo "DIFF  $rel mode=$m" >> "$OUT/summary.txt"
      echo "==== $rel mode=$m ====" >> "$OUT/diffs/$tag.diff"
      diff "$OUT/oracle-$tag" "$OUT/cand-$tag" | head -60 >> "$OUT/diffs/$tag.diff" 2>&1
    fi
  done
  # SAX1 (expat-compat surface) on the smaller docs, a few plans.
  if [ "$(stat -c %s "$doc")" -le 4000 ]; then
    for m in b1 b5 r9; do
      total=$((total + 1))
      tag=$(echo "$rel" | tr '/' '_')__sax1_$m
      "$OUT/probe-oracle" -s sax1 "$m" "$doc" > "$OUT/oracle-$tag" 2>&1
      "$OUT/probe-cand"   -s sax1 "$m" "$doc" > "$OUT/cand-$tag"   2>&1
      if ! cmp -s "$OUT/oracle-$tag" "$OUT/cand-$tag"; then
        fail=$((fail + 1))
        echo "DIFF  $rel mode=sax1/$m" >> "$OUT/summary.txt"
        echo "==== $rel mode=sax1/$m ====" >> "$OUT/diffs/$tag.diff"
        diff "$OUT/oracle-$tag" "$OUT/cand-$tag" | head -60 >> "$OUT/diffs/$tag.diff" 2>&1
      fi
    done
  fi
done

echo "cells=$total diffs=$fail" >> "$OUT/summary.txt"
echo "=== summary ==="
cat "$OUT/summary.txt"
if [ "$fail" -ne 0 ]; then
  echo "PUSHDIFF: $fail/$total cells diverge (see diffs/)"
  exit 1
fi
echo "PUSHDIFF: all $total cells byte-identical (oracle == candidate)"
