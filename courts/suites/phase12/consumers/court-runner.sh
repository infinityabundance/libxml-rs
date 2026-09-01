#!/usr/bin/env bash
#
# court-runner.sh — Phase 12 EXTERNAL-CONSUMERS court.
#
# Court family: EXTERNAL-CONSUMERS
#
# Compiles the UPSTREAM libxml2-2.15.0 example programs UNMODIFIED against
# the candidate headers + DSOs and against the system oracle, runs each in a
# workdir containing the upstream test documents, and requires byte-identical
# stdout/stderr/exit. These programs are real external C consumers the
# project did not write — the Phase-12 "unmodified external consumers"
# requirement.
#
# Evidence: courts/receipts/phase-12/consumers-<ts>.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
LIBDIR="${ARTIFACT}/lib"
INCLUDE="${PROJECT_DIR}/include"
SRCDIR="${PROJECT_DIR}/oracle/historical/src/libxml2-2.15.0/example"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-12"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
RECEIPT="${RECEIPT_DIR}/consumers-${TS}.json"
WORK="$(mktemp -d /tmp/consumers.XXXXXX)"
mkdir -p "$RECEIPT_DIR"

results=()
pass=0
fail=0
declare -A FAILED_MSGS
record() {
    results+=("{\"item\":\"$2\",\"status\":\"$1\"}")
    if [ "$1" = "PASS" ]; then pass=$((pass+1)); else fail=$((fail+1)); FAILED_MSGS["$2"]="${3:-}"; fi
}
sanitize() { echo "$1" | tr -d '"\r' | tr '\n' ' '; }

# upstream test documents -> workdir
for f in test1.xml test2.xml test3.xml; do
    cp -f "${SRCDIR}/${f}" "$WORK/" 2>/dev/null
done
# tree1 parses a file passed as argv[1]; provide the upstream test doc
cp -f "${SRCDIR}/test1.xml" "$WORK/file.xml" 2>/dev/null

# case table: program|args
CASES=(
    "parse1|test1.xml"
    "parse2|test1.xml"
    "parse3|"
    "parse4|test1.xml"
    "reader1|test1.xml"
    "reader2|test1.xml"
    "reader3|test1.xml"
    "reader4|test1.xml"
    "tree1|file.xml"
    "tree2|"
    "xpath1|test3.xml '//child2'"
    "xpath2|test3.xml '//discarded' discarded"
    "io1|"
    "io2|"
    "testWriter|"
)

for entry in "${CASES[@]}"; do
    prog="${entry%%|*}"
    args="${entry#*|}"
    src="${SRCDIR}/${prog}.c"
    [ -f "$src" ] || continue
    # oracle build
    if cc -std=c11 -I/usr/include/libxml2 "$src" -lxml2 -lm -o "$WORK/${prog}-oracle" \
            >"$WORK/cc-o.txt" 2>&1; then
        ( cd "$WORK" && "./${prog}-oracle" $args >"${prog}.o.out" 2>"${prog}.o.err" )
        o_rc=$?
    else
        record FAIL "consumer:$prog:oracle-build" "$(sanitize "$(head -3 "$WORK/cc-o.txt")")"
        continue
    fi
    # candidate build (unmodified source, candidate headers + DSOs)
    if cc -std=c11 -I"${INCLUDE}" "$src" -L"${ARTIFACT}" -lxml2 \
            -Wl,-rpath,"${ARTIFACT}" -lm -o "$WORK/${prog}-cand" \
            >"$WORK/cc-c.txt" 2>&1; then
        ( cd "$WORK" && "./${prog}-cand" $args >"${prog}.c.out" 2>"${prog}.c.err" )
        c_rc=$?
    else
        record FAIL "consumer:$prog:candidate-build" "$(sanitize "$(head -3 "$WORK/cc-c.txt")")"
        continue
    fi
    if [ "$o_rc" = "$c_rc" ] && cmp -s "$WORK/${prog}.o.out" "$WORK/${prog}.c.out" \
            && cmp -s "$WORK/${prog}.o.err" "$WORK/${prog}.c.err"; then
        record PASS "consumer:$prog (byte-identical, rc=$c_rc)"
    else
        record FAIL "consumer:$prog" "oracle-rc=$o_rc candidate-rc=$c_rc out-diff=$(cmp -s "$WORK/${prog}.o.out" "$WORK/${prog}.c.out"; echo $?) err-diff=$(cmp -s "$WORK/${prog}.o.err" "$WORK/${prog}.c.err"; echo $?)"
    fi
done

# ── receipt ──────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"EXTERNAL-CONSUMERS\","
    echo "  \"phase\": \"12\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"schema\": \"external-consumers-1\","
    echo "  \"source\": \"oracle/historical/src/libxml2-2.15.0/example (unmodified)\","
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
echo "WORK=$WORK" > /tmp/court-work.txt
echo "receipt -> $RECEIPT"
echo "passed=$pass failed=$fail"
exit $fail
