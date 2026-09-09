#!/bin/bash
# pushdiff-differential.sh — §16.7.8 push-parser per-call differential, runs
# INSIDE the oracle court image (libxml-rs/phase14-debian:1).
#
# Compiles pushdiff-probe.c against the ORACLE (/usr/local) and the
# CANDIDATE (/candidate), then feeds every corpus document to
# xmlCreatePushParserCtxt + xmlParseChunk under fixed and random chunking
# plans. The probe prints ONE merged, deterministic trace per (file, mode):
# per-call rc/errNo/wellFormed/instate/cursor (input offset, line, col,
# inputNr, nameNr) interleaved with every SAX event and structured error
# record in invocation order.
#
# Plans cover every lifecycle entrance:
#   bN / rS    fixed / random chunkings (incl. single-byte: every byte
#              offset becomes a chunk boundary)
#   C<plan>    constructor-initial chunk (first split goes to
#              xmlCreatePushParserCtxt)
#   <plan>zK   K zero-length non-final calls injected after each real chunk
#   Rz<plan>   two-document reset cell: parse A, xmlCtxtResetPush(NULL,0),
#              parse B — stale state must not leak across the reset
#   Ri<plan>   two-document reset cell: parse A, xmlCtxtResetPush(whole B)
#   -S K       xmlStopParser after the K-th start element; later chunks
#              must be refused with the recorded error
#
# The probe is compiled with -Wall -Wextra -Werror against each provider's
# own headers, so every callback prototype is exactly the provider's
# (no ABI drift, no -w). Each provider cell runs under a bounded timeout;
# a timeout/crash is a first-class differential failure, never a hang.
#
# Gate G — oracle trace == candidate trace, byte-for-byte, for every
# (document, chunking plan, sax mode). The ground-truth gate the
# incremental push parser (16.7.8) must keep green.
#
# Env: GEN, PROBE_SRC, FIXTURES, OUT, CELL_TIMEOUT (defaults match the
# host launcher).
set -uo pipefail
export LC_ALL=C
GEN="${GEN:-/court/suites/phase16/gen_pushdiff_corpus.py}"
PROBE_SRC="${PROBE_SRC:-/court/suites/phase16/pushdiff-probe.c}"
FIXTURES="${FIXTURES:-/court/suites/phase14/sources}"
OUT="${OUT:-/scanout}"
CELL_TIMEOUT="${CELL_TIMEOUT:-180}"

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

# Compile the probe against both providers — strict: a single warning in
# the recorder ABI is a build failure, not a suppressed risk.
cc -O1 -Wall -Wextra -Werror -o "$OUT/probe-oracle" "$PROBE_SRC" \
  -I/usr/local/include/libxml2 -L/usr/local/lib -lxml2 -Wl,-rpath,/usr/local/lib || exit 1
cc -O1 -Wall -Wextra -Werror -o "$OUT/probe-cand" "$PROBE_SRC" \
  -I/candidate/include/libxml2 -L/candidate/lib -lxml2 -Wl,-rpath,/candidate/lib || exit 1

# Pick chunking plans by document size so single-byte chunking stays
# affordable while every byte offset is still covered across the corpus.
pick_modes() { # path -> echoes the sax2 modes to run
  local sz
  sz=$(stat -c %s "$1")
  if   [ "$sz" -le 200 ];  then echo "b1 b2 b3 b5 b7 b16 b257 r3 r9 r77 Cb1 Cb257 Cr9 b1z2"
  elif [ "$sz" -le 2000 ]; then echo "b1 b2 b5 b64 b257 r1 r42 Cb5 Cr42 b64z2"
  elif [ "$sz" -le 9216 ]; then echo "b1 b7 b64 b1024 r2 r17 Cb64 r17z1"   # 9 KiB
  elif [ "$sz" -le 65536 ]; then echo "b16 b4096 b32768 r5 r23"
  else echo "b4096 b65536 r11"
  fi
}

fail=0
total=0
: > "$OUT/summary.txt"

run_cell() { # tag extra-args mode doc... -> 0 identical / 1 diverge-or-crash
  local tag="$1" extra="$2" m="$3"; shift 3
  local orc ccrc
  timeout "$CELL_TIMEOUT" "$OUT/probe-oracle" $extra "$m" "$@" > "$OUT/oracle-$tag" 2>&1
  orc=$?
  timeout "$CELL_TIMEOUT" "$OUT/probe-cand"   $extra "$m" "$@" > "$OUT/cand-$tag"   2>&1
  ccrc=$?
  if [ $orc -ne 0 ] || [ $ccrc -ne 0 ]; then
    echo "TIMEOUT/CRASH $tag oracle_rc=$orc cand_rc=$ccrc" >> "$OUT/summary.txt"
    return 1
  fi
  if ! cmp -s "$OUT/oracle-$tag" "$OUT/cand-$tag"; then
    return 1
  fi
  return 0
}

record_fail() { # rel-tag mode-label tag
  local rel="$1" ml="$2" tag="$3"
  fail=$((fail + 1))
  echo "DIFF  $rel mode=$ml" >> "$OUT/summary.txt"
  echo "==== $rel mode=$ml ====" >> "$OUT/diffs/$tag.diff"
  diff "$OUT/oracle-$tag" "$OUT/cand-$tag" | head -60 >> "$OUT/diffs/$tag.diff" 2>&1
}

for doc in "${DOCS[@]}"; do
  rel=${doc#$OUT/}
  modes=$(pick_modes "$doc")
  for m in $modes; do
    total=$((total + 1))
    tag=$(echo "$rel" | tr '/' '_')__$m
    if ! run_cell "$tag" "" "$m" "$doc"; then
      record_fail "$rel" "$m" "$tag"
    fi
  done
  # SAX1 (expat-compat surface) on the smaller docs, a few plans.
  if [ "$(stat -c %s "$doc")" -le 4000 ]; then
    for m in b1 b5 r9; do
      total=$((total + 1))
      tag=$(echo "$rel" | tr '/' '_')__sax1_$m
      if ! run_cell "$tag" "-s sax1" "$m" "$doc"; then
        record_fail "$rel" "sax1/$m" "$tag"
      fi
    done
  fi
done

# xmlCtxtResetPush cells: pairs of shape-different documents (DTD ↔ ns,
# CR line endings ↔ attr-heavy, error doc ↔ cdata, …) in both directions.
reset_pairs="doctype-subset-choice:ns-rescope ns-rescope:doctype-subset-choice \
text-cr-chunkend:attr-many unclosed-root2:content-cdata \
content-cdata:doctype-entity-nested attr-unicode:text-unicode-emoji-tail"
for pair in $reset_pairs; do
  a=${pair%%:*}
  b=${pair##*:}
  for m in Rzb257 Rzb1 Rib257; do
    total=$((total + 1))
    tag="reset_${a}_${b}__$m"
    if ! run_cell "$tag" "" "$m" "$OUT/corpus/$a.xml" "$OUT/corpus/$b.xml"; then
      record_fail "reset $a -> $b" "$m" "$tag"
    fi
  done
done

# xmlStopParser cells: stop after the K-th start element, then refuse.
for cell in "deep:b16:1" "ns-rescope:b7:2" "empty-elements:b3:3" "mismatch:b5:1"; do
  docn=${cell%%:*}; rest=${cell#*:}; m=${rest%%:*}; k=${rest##*:}
  total=$((total + 1))
  tag="stop_${docn}__${m}_s${k}"
  if ! run_cell "$tag" "-S $k" "$m" "$OUT/corpus/$docn.xml"; then
    record_fail "stop $docn (K=$k)" "$m -S $k" "$tag"
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
