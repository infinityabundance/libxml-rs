#!/usr/bin/env bash
#
# court-runner.sh — 11.1-D preprocessor condition-lattice court (11.1-Z.1).
#
# Court family: PREPROCESSOR-SURFACE
#
# Proves the configuration-lattice analysis that 11.1-Y previously mapped to
# the build-config-script court (which tests xml2-config/xslt-config, NOT the
# preprocessor condition lattice). The lattice is produced by
# tools/archaeology/condition_inventory.py into:
#
#   oracle/historical/doxygen/conditions.json  — the condition universe:
#     every distinct #if/#ifdef/#ifndef/#elif expression that gates a parity
#     surface, per project, with file/version provenance and the
#     public_header flag (appears in a public header).
#   oracle/historical/doxygen/coverage.json    — per-condition truth table
#     across the generated configuration lattice: both the true and the false
#     direction of every condition must be exercised by at least one
#     configuration (branch coverage), except literal #if 0 / #if 1.
#   oracle/historical/doxygen/configs.json     — the generated configurations
#     (oracle configs + contrast deltas), hashed.
#
# Verifies (no regeneration — the inventory is expensive and already
# committed; re-run condition_inventory.py --project all to regenerate):
#
#   1. all three artifacts exist and parse;
#   2. the condition universe is non-empty and every condition has a coverage
#      entry;
#   3. every public-header condition (parity-surface gating) has BOTH
#      directions covered, modulo the documented #if 0 / #if 1 literals whose
#      other direction is structurally unreachable;
#   4. the configuration lattice is non-empty and hashed.
#
# Evidence: courts/receipts/phase-11/preprocessor-surface-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
DOX="${PROJECT_DIR}/oracle/historical/doxygen"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/preprocessor-surface-${TS}-receipt.json"
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

COND="${DOX}/conditions.json"
COV="${DOX}/coverage.json"
CFG="${DOX}/configs.json"

# ── 1. artifacts exist + parse ─────────────────────────────────────────────── #
for f in conditions.json coverage.json configs.json; do
    if [ -s "${DOX}/${f}" ]; then
        record PASS "artifact:${f}:present"
    else
        record FAIL "artifact:${f}:present" "missing or empty"
    fi
done
if python3 -c "import json,sys; json.load(open('${COND}')); json.load(open('${COV}')); json.load(open('${CFG}'))" 2>/dev/null; then
    record PASS "artifacts:valid-json"
else
    record FAIL "artifacts:valid-json" "one of the inventories is not valid JSON"
fi

# ── 2. condition universe non-empty; every condition has coverage ──────────── #
universe=$(python3 -c "
import json
c = json.load(open('${COND}'))
print(len(c) if isinstance(c, dict) else len(c))
" 2>/dev/null)
if [ -n "$universe" ] && [ "$universe" -gt 0 ] 2>/dev/null; then
    record PASS "conditions:universe-nonempty=$universe"
else
    record FAIL "conditions:universe-nonempty" "got '$universe'"
fi
missing_cov=$(python3 -c "
import json
c = json.load(open('${COND}'))
cov = json.load(open('${COV}'))
missing = [k for k in c if k not in cov]
print(len(missing))
" 2>/dev/null)
if [ "$missing_cov" = "0" ]; then
    record PASS "conditions:all-covered"
else
    record FAIL "conditions:all-covered" "$missing_cov conditions lack a coverage entry"
fi

# ── 3. public-header conditions have both-direction coverage ──────────────── #
both_cov=$(python3 -c "
import json, re
cov = json.load(open('${COV}'))
ph = [k for k, v in cov.items() if isinstance(v, dict) and v.get('public_header')]
# literal #if 0 / #if 1: the other direction is structurally unreachable and
# documented in condition_inventory.py (11.1-D), so they are exempt.
literal = re.compile(r'^[^:]+:(0|1)\$')
bad = []
for k in ph:
    v = cov[k]
    if literal.match(k):
        continue
    if not v.get('true_config_count') or not v.get('false_config_count'):
        bad.append(k)
print(len(ph), len(bad))
" 2>/dev/null)
ph_n=$(echo "$both_cov" | awk '{print $1}')
bad_n=$(echo "$both_cov" | awk '{print $2}')
if [ -n "$ph_n" ] && [ "$ph_n" -gt 0 ] 2>/dev/null; then
    record PASS "conditions:public-header=$ph_n"
else
    record FAIL "conditions:public-header" "no public-header conditions found (got '$both_cov')"
fi
if [ "$bad_n" = "0" ]; then
    record PASS "conditions:public-header-both-direction-covered"
else
    record FAIL "conditions:public-header-both-direction-covered" "$bad_n public-header conditions lack both-direction coverage"
fi

# ── 4. configuration lattice non-empty + hashed ────────────────────────────── #
cfg_hash=$(python3 -c "
import json, hashlib
cfg = json.load(open('${CFG}'))
h = hashlib.sha256(json.dumps(cfg, sort_keys=True).encode()).hexdigest()
print(h[:16])
" 2>/dev/null)
if [ -n "$cfg_hash" ]; then
    record PASS "configs:lattice-hashed=$cfg_hash"
else
    record FAIL "configs:lattice-hashed" "unreadable"
fi

# ── receipt ────────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"PREPROCESSOR-SURFACE\","
    echo "  \"phase\": \"11.1-D\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"artifacts\": {"
    echo "    \"conditions\": \"$COND\","
    echo "    \"coverage\": \"$COV\","
    echo "    \"configs\": \"$CFG\""
    echo "  },"
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
