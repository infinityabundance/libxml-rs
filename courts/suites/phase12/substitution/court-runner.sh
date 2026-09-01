#!/usr/bin/env bash
#
# court-runner.sh — Phase 12 STATIC-SUBSTITUTION + PKG-CONFIG/CONFIG-SCRIPT
# substitution court.
#
# Court family: STATIC-SUBSTITUTION / PKG-CONFIG-SUBSTITUTION
#
#   static       — an unmodified C consumer linked ONLY against the candidate
#                  staticlibs (libxml2.a / libxslt.a / libexslt.a) runs
#                  standalone with no candidate shared object anywhere;
#   pkg-config   — the same consumer built through `pkg-config --cflags
#                  --libs libxml-2.0|libxslt|libexslt` against the candidate
#                  .pc files compiles, links and runs against the candidate
#                  DSOs;
#   config-script— the same consumer built through the candidate's
#                  xml2-config / xslt-config scripts compiles, links and runs.
#
# Evidence: courts/receipts/phase-12/substitution-<ts>.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
LIBDIR="${ARTIFACT}/lib"
INCLUDE="${PROJECT_DIR}/include"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-12"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
RECEIPT="${RECEIPT_DIR}/substitution-${TS}.json"
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

cat > /tmp/sub-consumer.c <<'CEOF'
/* Unmodified external C consumer (upstream-style API use). */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>
#include <libexslt/exslt.h>

static const char XSL[] =
    "<?xml version='1.0'?><xsl:stylesheet version='1.0'"
    " xmlns:xsl='http://www.w3.org/1999/XSL/Transform'>"
    "<xsl:template match='/'>ok-<xsl:value-of select='/r/@n'/></xsl:template>"
    "</xsl:stylesheet>";

int main(void) {
    xmlDocPtr d = xmlReadMemory("<r n='5'/>", 10, "t.xml", NULL, 0);
    if (!d) return 1;
    xmlDocPtr s = xmlReadMemory(XSL, (int)strlen(XSL), "t.xsl", NULL, 0);
    xsltStylesheetPtr ss = s ? xsltParseStylesheetDoc(s) : NULL;
    if (!ss) return 2;
    xmlDocPtr r = xsltApplyStylesheet(ss, d, NULL);
    if (!r) return 3;
    xmlChar *out = NULL;
    int len = 0;
    xsltSaveResultToString(&out, &len, r, ss);
    exsltRegisterAll();
    printf("consumer %s ver=%d %d %d %d\n",
           out ? (char *)out : "(null)",
           xsltLibxsltVersion, exsltLibexsltVersion, exsltLibxmlVersion,
           exsltLibxsltVersion);
    xmlFree(out);
    xmlFreeDoc(r);
    xsltFreeStylesheet(ss);
    xmlFreeDoc(d);
    xmlCleanupParser();
    return 0;
}
CEOF

# ── 1. STATIC substitution: no shared candidate object anywhere ──────────── #
for pkg in libxml2 libxslt libexslt; do
    if [ -f "${LIBDIR}/${pkg}.a" ]; then
        record PASS "static:${pkg}.a present"
    else
        record FAIL "static:${pkg}.a present" "missing ${LIBDIR}/${pkg}.a"
    fi
done
if cc -std=c11 -I"${INCLUDE}/libxml2" -I"${INCLUDE}" /tmp/sub-consumer.c \
        "${LIBDIR}/libexslt.a" "${LIBDIR}/libxslt.a" "${LIBDIR}/libxml2.a" \
        -ldl -lpthread -lm -o /tmp/sub-static >/tmp/sub-cc1.txt 2>&1; then
    record PASS "static:build"
else
    record FAIL "static:build" "$(sanitize "$(head -3 /tmp/sub-cc1.txt)")"
fi
if [ -x /tmp/sub-static ]; then
    if readelf -d /tmp/sub-static 2>/dev/null | grep -q NEEDED; then
        needs="$(readelf -d /tmp/sub-static | grep NEEDED | tr '\n' ' ')"
        if echo "$needs" | grep -qE "libxml2|libxslt|libexslt"; then
            record FAIL "static:no-shared-candidate-deps" "$(sanitize "$needs")"
        else
            record PASS "static:no-shared-candidate-deps (fully static)"
        fi
    else
        record PASS "static:no-shared-candidate-deps (fully static)"
    fi
    out="$(/tmp/sub-static 2>&1)"
    if echo "$out" | grep -q "ok-5"; then
        record PASS "static:links-and-runs-standalone"
    else
        record FAIL "static:links-and-runs-standalone" "$(sanitize "$out")"
    fi
fi

# ── 2. PKG-CONFIG substitution (candidate .pc files) ─────────────────────── #
PKG_CONFIG_PATH="${LIBDIR}/pkgconfig"
if command -v pkg-config >/dev/null 2>&1; then
    for pc in libxml-2.0 libxslt libexslt; do
        if PKG_CONFIG_PATH="$PKG_CONFIG_PATH" pkg-config --exists "$pc" 2>/dev/null; then
            record PASS "pkgconfig:$pc exists"
        else
            record FAIL "pkgconfig:$pc exists" "not found via $PKG_CONFIG_PATH"
        fi
    done
    cflags="$(PKG_CONFIG_PATH="$PKG_CONFIG_PATH" pkg-config --cflags libxml-2.0 libxslt libexslt 2>/dev/null)"
    libs="$(PKG_CONFIG_PATH="$PKG_CONFIG_PATH" pkg-config --libs libxml-2.0 libxslt libexslt 2>/dev/null)"
    if cc -std=c11 $cflags /tmp/sub-consumer.c $libs \
            -Wl,-rpath,"${ARTIFACT}" -o /tmp/sub-pc >/tmp/sub-cc2.txt 2>&1; then
        record PASS "pkgconfig:build"
        out="$(LD_LIBRARY_PATH="${ARTIFACT}" /tmp/sub-pc 2>&1)"
        if echo "$out" | grep -q "ok-5"; then
            record PASS "pkgconfig:links-and-runs"
        else
            record FAIL "pkgconfig:links-and-runs" "$(sanitize "$out")"
        fi
    else
        record FAIL "pkgconfig:build" "$(sanitize "$(head -3 /tmp/sub-cc2.txt)")"
    fi
else
    record FAIL "pkgconfig:tool-present" "pkg-config not installed"
fi

# ── 3. CONFIG-SCRIPT substitution (xml2-config / xslt-config) ────────────── #
XML2CFG="${ARTIFACT}/bin/xml2-config"
XSLTCFG="${ARTIFACT}/bin/xslt-config"
if [ -x "$XML2CFG" ] && [ -x "$XSLTCFG" ]; then
    xcflags="$("$XML2CFG" --cflags) $("$XSLTCFG" --cflags)"
    xlibs="$("$XML2CFG" --libs) $("$XSLTCFG" --libs)"
    # xml2-config/xslt-config do not emit -lexslt (upstream behavior); a
    # consumer using EXSLT adds it itself, as here.
    if cc -std=c11 $xcflags /tmp/sub-consumer.c $xlibs -L"${ARTIFACT}" -lexslt \
            -Wl,-rpath,"${ARTIFACT}" -o /tmp/sub-cfg >/tmp/sub-cc3.txt 2>&1; then
        record PASS "config-script:build"
        out="$(LD_LIBRARY_PATH="${ARTIFACT}" /tmp/sub-cfg 2>&1)"
        if echo "$out" | grep -q "ok-5"; then
            record PASS "config-script:links-and-runs"
        else
            record FAIL "config-script:links-and-runs" "$(sanitize "$out")"
        fi
    else
        record FAIL "config-script:build" "$(sanitize "$(head -3 /tmp/sub-cc3.txt)")"
    fi
else
    record FAIL "config-script:scripts-present" "missing xml2-config/xslt-config"
fi

# ── receipt ──────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"STATIC-SUBSTITUTION\","
    echo "  \"phase\": \"12\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"schema\": \"substitution-1\","
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
