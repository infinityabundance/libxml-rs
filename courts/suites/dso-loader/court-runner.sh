#!/usr/bin/env bash
#
# court-runner.sh — 11.1-T dynamic-loader court.
#
# Court family: DSO-LOADER
#
# Tests the candidate as an actual DSO through the dynamic loader, using an
# external C program that dlopen()s the upstream runtime names and dlsym()s
# everything — no compile-time dependency on the headers at all. Verifies:
#
#   1. dlopen("libxml2.so.16") / ("libxslt.so.1") / ("libexslt.so.0")
#      resolve through the SONAME chains to the candidate DSO;
#   2. symbol presence + callability (functions) and readability (data);
#   3. callback registration fires (xmlSetStructuredErrorFunc);
#   4. CANDIDATE IDENTITY: dladdr() reports the loaded object inside the
#      artifact directory — never a system libxml2 (contamination guard);
#   5. ELF contract of the DSO itself: SONAME == libxml2.so.16, and NEEDED
#      contains no libxml2/libxslt/libexslt (candidate independence);
#   6. ldd of a consumer binary resolves libxml2.so.16 into the artifact;
#   7. symbol-TYPE parity (nm T/D/B/R) for a curated surface against the
#      oracle DSOs (incl. the R-000167 version data symbols: xsltLibxsltVersion
#      R, xsltEngineVersion D, exsltLibexsltVersion/exsltLibxmlVersion/
#      exsltLibxsltVersion R, exsltLibraryVersion D);
#   8. static linking through lib/libxml2.a runs without any shared object.
#
# Evidence: courts/receipts/phase-11/dso-loader-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
LIBDIR="${ARTIFACT}/lib"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/dso-loader-${TS}-receipt.json"
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

DSO="${ARTIFACT}/liblibxml_rs.so"
PROBE_BIN="/tmp/dso-probe"

# ── build the opaque probe (no headers, only -ldl) ─────────────────────────── #
if cc "${SCRIPT_DIR}/dso-probe.c" -ldl -o "$PROBE_BIN" >/tmp/dso-cc.txt 2>&1; then
    record PASS "probe:build"
else
    record FAIL "probe:build" "$(sanitize "$(head -3 /tmp/dso-cc.txt)")"
fi

# ── run the probe against the candidate via LD_LIBRARY_PATH ────────────────── #
run_probe() {
    label="$1"; ldp="$2"
    out="$(LD_LIBRARY_PATH="${ldp}" "$PROBE_BIN" "$ARTIFACT" 2>&1)"
    if printf '%s' "$out" | grep -q "VERDICT PASS"; then
        record PASS "$label"
    else
        record FAIL "$label" "$(sanitize "$(printf '%s' "$out" | grep -E 'FAIL|dlerror' | head -3)")"
    fi
    # contamination guard: the loaded object must be inside the artifact
    if printf '%s' "$out" | grep -q "loaded-object="; then
        obj="$(printf '%s' "$out" | grep 'loaded-object=' | sed 's/.*loaded-object=//')"
        case "$obj" in
            "$ARTIFACT"*) record PASS "$label:loaded-object-in-artifact" ;;
            *) record FAIL "$label:loaded-object-in-artifact" "loaded $obj" ;;
        esac
    fi
}
run_probe "probe:run:top-level-LD_LIBRARY_PATH" "${ARTIFACT}"
run_probe "probe:run:lib-LD_LIBRARY_PATH" "${LIBDIR}"

# ── ELF contract of the DSO ────────────────────────────────────────────────── #
soname="$(readelf -d "$DSO" 2>/dev/null | grep SONAME | sed 's/.*\[\(.*\)\]/\1/')"
if [ "$soname" = "libxml2.so.16" ]; then
    record PASS "elf:soname=libxml2.so.16"
else
    record FAIL "elf:soname" "got '$soname'"
fi
if readelf -d "$DSO" 2>/dev/null | grep NEEDED | grep -qE "libxml2|libxslt|libexslt"; then
    record FAIL "elf:no-hidden-libxml2-dep" "$(readelf -d "$DSO" | grep NEEDED)"
else
    record PASS "elf:no-hidden-libxml2-dep"
fi

# ── ldd of a linked consumer resolves into the artifact ────────────────────── #
cat > /tmp/dso-need.c <<'CEOF'
#include <libxml/parser.h>
int main(void){ xmlDocPtr d = xmlReadMemory("<a/>", 4, "t.xml", NULL, 0); if(d) xmlFreeDoc(d); return 0; }
CEOF
if cc -I"${ARTIFACT}/include/libxml2" /tmp/dso-need.c -L"${LIBDIR}" -lxml2 -o /tmp/dso-need >/tmp/dso-cc2.txt 2>&1; then
    needed="$(readelf -d /tmp/dso-need | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
    if printf '%s' "$needed" | grep -q "libxml2.so.16"; then
        record PASS "consumer:NEEDED=libxml2.so.16"
    else
        record FAIL "consumer:NEEDED" "got: $needed"
    fi
    ldd_out="$(LD_LIBRARY_PATH="${ARTIFACT}" ldd /tmp/dso-need)"
    if printf '%s' "$ldd_out" | grep "libxml2.so.16" | grep -q "$ARTIFACT"; then
        record PASS "consumer:ldd-resolves-into-artifact"
    else
        record FAIL "consumer:ldd-resolves-into-artifact" "$(printf '%s' "$ldd_out" | grep libxml2)"
    fi
else
    record FAIL "consumer:build" "$(sanitize "$(head -3 /tmp/dso-cc2.txt)")"
fi

# ── symbol-TYPE parity vs oracle (curated surface) ─────────────────────────── #
sym_type() { # sym_type <dso> <name> — strip @@LIBXML2_x.y.z version suffixes
    nm -D --defined-only "$1" 2>/dev/null | awk -v n="$2" '{name=$3; sub(/@@.*/, "", name); if (name == n) {print $2; exit}}'
}
ORACLE_XML="/usr/lib/libxml2.so.16"
ORACLE_XSLT="/usr/lib/libxslt.so.1"
ORACLE_EXSLT="/usr/lib/libexslt.so.0"
type_mismatch=0
for spec in \
    "xmlReadMemory:T:${ORACLE_XML}" \
    "xmlNewDoc:T:${ORACLE_XML}" \
    "xmlParserVersion:D:${ORACLE_XML}" \
    "__xmlGenericError:D:${ORACLE_XML}" \
    "__xmlLastError:B:${ORACLE_XML}" \
    "xsltApplyStylesheet:T:${ORACLE_XSLT}" \
    "xsltLibxmlVersion:R:${ORACLE_XSLT}" \
    "xsltLibxsltVersion:R:${ORACLE_XSLT}" \
    "xsltEngineVersion:D:${ORACLE_XSLT}" \
    "exsltRegisterAll:T:${ORACLE_EXSLT}" \
    "exsltLibexsltVersion:R:${ORACLE_EXSLT}" \
    "exsltLibxmlVersion:R:${ORACLE_EXSLT}" \
    "exsltLibxsltVersion:R:${ORACLE_EXSLT}" \
    "exsltLibraryVersion:D:${ORACLE_EXSLT}" \
    "xmlXPathEval:T:${ORACLE_XML}"; do
    name="${spec%%:*}"; rest="${spec#*:}"; want="${rest%%:*}"; odso="${rest#*:}"
    t_cand="$(sym_type "$DSO" "$name")"
    t_orac="$(sym_type "$odso" "$name")"
    if [ "$t_cand" = "$t_orac" ]; then
        record PASS "symtype:$name=$t_cand"
    else
        record FAIL "symtype:$name" "candidate $t_cand vs oracle $t_orac"
        type_mismatch=1
    fi
done
if [ "$type_mismatch" -eq 1 ]; then
    record FAIL "symtype:summary" "see individual failures"
fi

# ── static linking through lib/libxml2.a ───────────────────────────────────── #
cat > /tmp/dso-static.c <<'SEOF'
#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
int main(void){
    xmlDocPtr d = xmlReadMemory("<root><x>1</x></root>", 20, "t.xml", NULL, 0);
    if (!d) return 1;
    xmlChar *mem = NULL; int len = 0;
    xmlDocDumpFormatMemory(d, &mem, &len, 1);
    printf("static-ok %s", (char *)(mem ? mem : (xmlChar *)""));
    if (mem) xmlFree(mem);
    xmlFreeDoc(d);
    return 0;
}
SEOF
if cc -I"${ARTIFACT}/include/libxml2" /tmp/dso-static.c "${LIBDIR}/libxml2.a" -ldl -lpthread -lm -o /tmp/dso-static >/tmp/dso-cc3.txt 2>&1; then
    out="$(/tmp/dso-static 2>&1)"
    if printf '%s' "$out" | grep -q "static-ok" && printf '%s' "$out" | grep -q "<root>"; then
        record PASS "static:libxml2.a-link-and-run"
    else
        record FAIL "static:libxml2.a-link-and-run" "$(sanitize "$out")"
    fi
else
    record FAIL "static:libxml2.a-link-and-run" "$(sanitize "$(head -3 /tmp/dso-cc3.txt)")"
fi

# ── receipt ────────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"DSO-LOADER\","
    echo "  \"phase\": \"11.1-T\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"artifact\": \"$ARTIFACT\","
    echo "  \"dso\": \"$DSO\","
    echo "  \"soname\": \"$soname\","
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
