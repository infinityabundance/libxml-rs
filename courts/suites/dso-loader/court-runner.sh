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
#      resolve through the SONAME chains to the candidate DSOs;
#   2. symbol presence + callability (functions) and readability (data);
#   3. callback registration fires (xmlSetStructuredErrorFunc);
#   4. CANDIDATE IDENTITY: dladdr() reports the loaded object inside the
#      artifact directory — never a system libxml2 (contamination guard);
#   5. ELF contract of the three DSOs (11.1-Z.1): the core DSO carries SONAME
#      libxml2.so.16 with no libxml2/libxslt/libexslt NEEDED; the two facade
#      DSOs are REAL files (distinct hashes) with SONAMEs libxslt.so.1 and
#      libexslt.so.0 and the upstream NEEDED chain (libxslt -> libxml2.so.16;
#      libexslt -> libxslt.so.1 + libxml2.so.16); facade export surfaces stay
#      namespace-scoped;
#   6. ldd of consumer binaries built with -lxml2 / -lxslt / -lexslt resolves
#      the right SONAME into the artifact, and each consumer links AND runs
#      against the candidate DSOs;
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

# ── ELF contract: three DSOs, three SONAMEs, upstream NEEDED chain ─────────── #
# 11.1-Z.1: the candidate now ships three REAL ELF DSOs (core + two post-link
# facades). The core carries SONAME libxml2.so.16 and stays self-contained;
# lib/libxslt.so.1.1.45 (SONAME libxslt.so.1) NEEDs libxml2.so.16; and
# lib/libexslt.so.0.8.25 (SONAME libexslt.so.0) NEEDs libxslt.so.1 and
# libxml2.so.16 — mirroring the oracle (readelf -d /usr/lib/libxslt.so.1,
# /usr/lib/libexslt.so.0).
XSLT_FACADE="${LIBDIR}/libxslt.so.1.1.45"
EXSLT_FACADE="${LIBDIR}/libexslt.so.0.8.25"

soname="$(readelf -d "$DSO" 2>/dev/null | grep SONAME | sed 's/.*\[\(.*\)\]/\1/')"
if [ "$soname" = "libxml2.so.16" ]; then
    record PASS "elf:core:soname=libxml2.so.16"
else
    record FAIL "elf:core:soname" "got '$soname'"
fi
if readelf -d "$DSO" 2>/dev/null | grep NEEDED | grep -qE "libxml2|libxslt|libexslt"; then
    record FAIL "elf:core:no-hidden-libxml2-dep" "$(readelf -d "$DSO" | grep NEEDED)"
else
    record PASS "elf:core:no-hidden-libxml2-dep"
fi

# distinct real DSOs: the facades are regular files (not symlinks to the
# core) and their hashes differ from the core's.
for f in "$XSLT_FACADE" "$EXSLT_FACADE"; do
    if [ -f "$f" ] && [ ! -L "$f" ]; then
        record PASS "elf:$(basename "$f"):real-file"
    else
        record FAIL "elf:$(basename "$f"):real-file" "missing or symlink"
    fi
    if [ "$(sha256sum "$f" | cut -d' ' -f1)" = "$(sha256sum "$DSO" | cut -d' ' -f1)" ]; then
        record FAIL "elf:$(basename "$f"):distinct-hash" "identical to core"
    else
        record PASS "elf:$(basename "$f"):distinct-hash"
    fi
done

xslt_soname="$(readelf -d "$XSLT_FACADE" 2>/dev/null | grep SONAME | sed 's/.*\[\(.*\)\]/\1/')"
if [ "$xslt_soname" = "libxslt.so.1" ]; then
    record PASS "elf:libxslt:soname=libxslt.so.1"
else
    record FAIL "elf:libxslt:soname" "got '$xslt_soname'"
fi
xslt_needed="$(readelf -d "$XSLT_FACADE" 2>/dev/null | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
if printf '%s' "$xslt_needed" | grep -q "libxml2.so.16"; then
    record PASS "elf:libxslt:NEEDED=libxml2.so.16"
else
    record FAIL "elf:libxslt:NEEDED" "got: $xslt_needed"
fi

exslt_soname="$(readelf -d "$EXSLT_FACADE" 2>/dev/null | grep SONAME | sed 's/.*\[\(.*\)\]/\1/')"
if [ "$exslt_soname" = "libexslt.so.0" ]; then
    record PASS "elf:libexslt:soname=libexslt.so.0"
else
    record FAIL "elf:libexslt:soname" "got '$exslt_soname'"
fi
exslt_needed="$(readelf -d "$EXSLT_FACADE" 2>/dev/null | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
if printf '%s' "$exslt_needed" | grep -q "libxslt.so.1" && printf '%s' "$exslt_needed" | grep -q "libxml2.so.16"; then
    record PASS "elf:libexslt:NEEDED=libxslt.so.1+libxml2.so.16"
else
    record FAIL "elf:libexslt:NEEDED" "got: $exslt_needed"
fi

# facade export surfaces stay namespace-scoped (version scripts): libxslt
# must not leak the whole combined core, and must actually export the xslt
# surface a consumer needs.
for f in "$XSLT_FACADE" "$EXSLT_FACADE"; do
    leaks="$(nm -D --defined-only "$f" 2>/dev/null | awk '{n=$3; sub(/@@.*/, "", n); print n}' | grep -vE '^(xslt|exslt|xslDebugStatus)$' | grep -E '^(xml|html|__xml)' | head -3)"
    if [ -z "$leaks" ]; then
        record PASS "elf:$(basename "$f"):namespace-scoped-exports"
    else
        record FAIL "elf:$(basename "$f"):namespace-scoped-exports" "leaked: $leaks"
    fi
done

# ── ldd of linked consumers resolves into the artifact (all three -l flows) ── #
cat > /tmp/dso-need.c <<'CEOF'
#include <libxml/parser.h>
int main(void){ xmlDocPtr d = xmlReadMemory("<a/>", 4, "t.xml", NULL, 0); if(d) xmlFreeDoc(d); return 0; }
CEOF
if cc -I"${ARTIFACT}/include/libxml2" /tmp/dso-need.c -L"${LIBDIR}" -lxml2 -o /tmp/dso-need >/tmp/dso-cc2.txt 2>&1; then
    needed="$(readelf -d /tmp/dso-need | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
    if printf '%s' "$needed" | grep -q "libxml2.so.16"; then
        record PASS "consumer:-lxml2:NEEDED=libxml2.so.16"
    else
        record FAIL "consumer:-lxml2:NEEDED" "got: $needed"
    fi
    ldd_out="$(LD_LIBRARY_PATH="${ARTIFACT}" ldd /tmp/dso-need)"
    if printf '%s' "$ldd_out" | grep "libxml2.so.16" | grep -q "$ARTIFACT"; then
        record PASS "consumer:-lxml2:ldd-resolves-into-artifact"
    else
        record FAIL "consumer:-lxml2:ldd-resolves-into-artifact" "$(printf '%s' "$ldd_out" | grep libxml2)"
    fi
else
    record FAIL "consumer:-lxml2:build" "$(sanitize "$(head -3 /tmp/dso-cc2.txt)")"
fi

# -lxslt consumer (pkg-config libxslt emits -lxslt -lxml2): the facade must
# satisfy xslt* at link time and the binary must NEED libxslt.so.1.
cat > /tmp/dso-need-xslt.c <<'XEOF'
#include <libxml/parser.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>
int main(void){
    xmlInitParser();
    const char *xsl = "<?xml version='1.0'?><xsl:stylesheet version='1.0' xmlns:xsl='http://www.w3.org/1999/XSL/Transform'><xsl:template match='/'>hi</xsl:template></xsl:stylesheet>";
    xmlDocPtr s = xmlReadMemory(xsl, (int)__builtin_strlen(xsl), "t.xsl", NULL, 0);
    xsltStylesheetPtr ss = xsltParseStylesheetDoc(s);
    xmlDocPtr r = xsltApplyStylesheet(ss, xmlNewDoc((const xmlChar *)"1.0"), NULL);
    xmlChar *out = NULL; int len = 0;
    xsltSaveResultToString(&out, &len, r, ss);
    if (out && out[0]) printf("XSLT-RUN-OK");
    xmlFree(out); xmlFreeDoc(r); xsltFreeStylesheet(ss); xmlCleanupParser();
    return 0;
}
XEOF
if cc -I"${ARTIFACT}/include/libxml2" -I"${ARTIFACT}/include" /tmp/dso-need-xslt.c \
        -L"${LIBDIR}" -lxslt -lxml2 -o /tmp/dso-need-xslt >/tmp/dso-cc4.txt 2>&1; then
    needed="$(readelf -d /tmp/dso-need-xslt | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
    if printf '%s' "$needed" | grep -q "libxslt.so.1" && printf '%s' "$needed" | grep -q "libxml2.so.16"; then
        record PASS "consumer:-lxslt:NEEDED=libxslt.so.1+libxml2.so.16"
    else
        record FAIL "consumer:-lxslt:NEEDED" "got: $needed"
    fi
    if LD_LIBRARY_PATH="${ARTIFACT}" /tmp/dso-need-xslt 2>&1 | grep -q "XSLT-RUN-OK"; then
        record PASS "consumer:-lxslt:links-and-runs"
    else
        record FAIL "consumer:-lxslt:links-and-runs" "$(sanitize "$(LD_LIBRARY_PATH="${ARTIFACT}" /tmp/dso-need-xslt 2>&1 | head -2)")"
    fi
else
    record FAIL "consumer:-lxslt:build" "$(sanitize "$(head -3 /tmp/dso-cc4.txt)")"
fi

# -lexslt consumer (pkg-config libexslt emits -lexslt -lxslt -lxml2): the
# exslt facade must satisfy exslt* at link time and the binary must NEED
# libexslt.so.0.
cat > /tmp/dso-need-exslt.c <<'EEOF'
#include <libexslt/exslt.h>
int main(void){
    exsltRegisterAll();
    printf("e=%d", exsltLibexsltVersion);
    return 0;
}
EEOF
if cc -I"${ARTIFACT}/include" /tmp/dso-need-exslt.c -L"${LIBDIR}" -lexslt -lxslt -lxml2 \
        -o /tmp/dso-need-exslt >/tmp/dso-cc5.txt 2>&1; then
    needed="$(readelf -d /tmp/dso-need-exslt | grep NEEDED | sed 's/.*\[\(.*\)\]/\1/' | tr '\n' ' ')"
    if printf '%s' "$needed" | grep -q "libexslt.so.0"; then
        record PASS "consumer:-lexslt:NEEDED=libexslt.so.0"
    else
        record FAIL "consumer:-lexslt:NEEDED" "got: $needed"
    fi
    if LD_LIBRARY_PATH="${ARTIFACT}" /tmp/dso-need-exslt 2>&1 | grep -q "e=825"; then
        record PASS "consumer:-lexslt:links-and-runs"
    else
        record FAIL "consumer:-lexslt:links-and-runs" "$(LD_LIBRARY_PATH="${ARTIFACT}" /tmp/dso-need-exslt 2>&1 | head -2)"
    fi
else
    record FAIL "consumer:-lexslt:build" "$(sanitize "$(head -3 /tmp/dso-cc5.txt)")"
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
    "xsltGenericDebug:D:${ORACLE_XSLT}" \
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
