#!/usr/bin/env bash
#
# court-runner.sh — 11.1-D Clang AST surface court (11.1-Z.1).
#
# Court family: AST-SURFACE
#
# Proves the Clang AST atlas that 11.1-Y previously mapped to
# header-compile (which compiles generated headers, NOT the AST surface).
# The atlas is produced by tools/archaeology/apiatlas.py into
# atlas/api/<project>/<version>.json for each oracle project, capturing the
# public API surface as parsed by Clang (functions, typedefs, records, enums,
# enumerators, callbacks, globals) plus the generator identity and the
# upstream version tag it was extracted from.
#
# Verifies:
#   1. every expected per-project atlas exists and parses;
#   2. each atlas records the apiatlas generator and its version tag;
#   3. each atlas carries a non-empty function surface (libexslt records the
#      smaller exslt surface by design, so the threshold is per-project);
#   4. the inventory is consumed by the parity matrix (the counts it feeds
#      appear in atlas/PARITY_MATRIX.json under oracle_clang_ast).
#
# Evidence: courts/receipts/phase-11/ast-surface-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
API_DIR="${PROJECT_DIR}/atlas/api"
ATLAS="${PROJECT_DIR}/atlas"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/ast-surface-${TS}-receipt.json"
mkdir -p "$RECEIPT_DIR"

results=()
pass=0
fail=0
declare -A FAILED_MSGS

record() {
    results+=("{\"item\":\"$2\",\"status\":\"$1\"}")
    if [ "$1" = "PASS" ]; then pass=$((pass+1)); else fail=$((fail+1)); FAILED_MSGS["$2"]="${3:-}"; fi
}

sanitize() {
    echo "$1" | tr -d '"\r' | tr '\n' ' '
}

# project:version-file:version_tag:min_functions
PROJECTS="libxml2:2.15.3:v2.15.3:1000 libxslt:1.1.45:v1.1.45:100 libexslt:0.8.25:v1.1.45:5"
for spec in $PROJECTS; do
    proj="${spec%%:*}"; rest="${spec#*:}"; ver="${rest%%:*}"
    rest="${rest#*:}"; tag="${rest%%:*}"; minfn="${rest#*:}"
    f="${API_DIR}/${proj}/${ver}.json"
    if [ -s "$f" ]; then
        record PASS "${proj}:atlas:present"
    else
        record FAIL "${proj}:atlas:present" "missing ${f}"
        continue
    fi
    gen="$(python3 -c "
import json,sys
d=json.load(open('${f}'))
print(d.get('generator',''))
print(d.get('version_tag',''))
print(len(d.get('functions') or []))
" 2>/dev/null)"
    g=$(echo "$gen" | sed -n 1p)
    v=$(echo "$gen" | sed -n 2p)
    n=$(echo "$gen" | sed -n 3p)
    if [ "$g" = "tools/archaeology/apiatlas.py" ]; then
        record PASS "${proj}:generator=apiatlas"
    else
        record FAIL "${proj}:generator=apiatlas" "got '$g'"
    fi
    if [ "$v" = "$tag" ]; then
        record PASS "${proj}:version_tag=$tag"
    else
        record FAIL "${proj}:version_tag" "got '$v', want '$tag'"
    fi
    if [ -n "$n" ] && [ "$n" -ge "$minfn" ] 2>/dev/null; then
        record PASS "${proj}:functions=$n"
    else
        record FAIL "${proj}:functions" "got '$n', want >= $minfn"
    fi
done

# the atlas feeds the parity matrix's oracle_clang_ast counts
if python3 -c "
import json
m = json.load(open('${ATLAS}/PARITY_MATRIX.json'))
ok = True
for proj in ('libxml2','libxslt','libexslt'):
    row = m['projects'][proj]['counts']['public_functions']
    if row.get('oracle_clang_ast') is None:
        ok = False
print(ok)
" 2>/dev/null | grep -q True; then
    record PASS "parity-matrix:consumes-clang-ast"
else
    record FAIL "parity-matrix:consumes-clang-ast" "oracle_clang_ast not populated"
fi

# ── receipt ────────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"AST-SURFACE\","
    echo "  \"phase\": \"11.1-D\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"atlas_dir\": \"$API_DIR\","
    echo "  \"cases\": ["
    for i in "${!results[@]}"; do
        sep=","
        [ "$i" -eq $(( ${#results[@]} - 1 )) ] && sep=""
        echo "    ${results[$i]}$sep"
    done
    echo "  ],"
    echo "  \"summary\": { \"passed\": $pass, \"failed\": $fail },"
    echo "  \"failures\": {"
    first=1
    for k in "${!FAILED_MSGS[@]}"; do
        [ $first -eq 1 ] || echo ","
        first=0
        echo "    \"$k\": \"${FAILED_MSGS[$k]}\""
    done
    echo "  }"
    echo "}"
} > "$RECEIPT"
echo "receipt -> $RECEIPT"
echo "passed=$pass failed=$fail"

exit $fail
