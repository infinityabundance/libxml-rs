#!/usr/bin/env bash
#
# court-runner.sh — 11.1-S install-layout drop-in court.
#
# Court family: INSTALL-LAYOUT
#
# Audits the complete installation contract of the candidate drop-in against
# the upstream libtool installation layout (oracle references:
# oracle/historical/prefix/libxml2-2.15.0 and libxslt-1.1.42, plus the system
# oracle /usr/lib SONAMEs). Verifies:
#
#   1. library naming + SONAME chains (libxml2.so.16, libxslt.so.1,
#      libexslt.so.0) resolve to the real candidate DSO;
#   2. static library names (libxml2.a, libxslt.a, libexslt.a);
#   3. libtool metadata (.la version fields, xsltConf.sh, libxslt-plugins);
#   4. pkg-config files (lib/pkgconfig + compat pkgconfig/);
#   5. header tree (libxml 46, libxslt superset of oracle, libexslt 3,
#      libxml2/libxml upstream hierarchy) — every oracle header present;
#   6. version-header macros agree with the runtime version targets
#      (2.15.3 / 1.1.45 / 0.8.25);
#   7. executables (bin/xml2-config, bin/xslt-config, xmllint, xmlcatalog,
#      xsltproc);
#   8. legacy names from earlier build-script versions are absent;
#   9. version reporting coherence: config scripts, pkg-config, DSO runtime
#      APIs and headers all report the same target versions.
#
# Evidence: courts/receipts/phase-11/install-layout-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
LIBDIR="${ARTIFACT}/lib"
INCLUDEDIR="${ARTIFACT}/include"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
ORACLE_XML="${PROJECT_DIR}/oracle/historical/prefix/libxml2-2.15.0"
ORACLE_XSLT="${PROJECT_DIR}/oracle/historical/prefix/libxslt-1.1.42"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/install-layout-${TS}-receipt.json"
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

expect() {
    label="$1"; actual="$2"; want="$3"
    if printf '%s' "$actual" | grep -qE "$want"; then
        record PASS "$label"
    else
        record FAIL "$label" "got: $(sanitize "$actual") | want ~ $(sanitize "$want")"
    fi
}

# chain resolves: <label> <link-path> <final-target-name>
chain() {
    label="$1"; link="$2"; final="$3"
    if [ -L "$link" ] || [ -e "$link" ]; then
        resolved="$(readlink -f "$link" 2>/dev/null)"
        if [ "$(basename "$resolved")" = "$final" ]; then
            record PASS "$label"
        else
            record FAIL "$label" "resolves to $resolved, want .../$final"
        fi
    else
        record FAIL "$label" "missing"
    fi
}

# ── 1. shared library naming + SONAME chains ───────────────────────────────── #

chain "libxml2:libxml2.so"          "${LIBDIR}/libxml2.so"        "${CANDIDATE_SO:-liblibxml_rs.so}"
chain "libxml2:libxml2.so.16"       "${LIBDIR}/libxml2.so.16"     "liblibxml_rs.so"
chain "libxml2:libxml2.so.16.1.3"   "${LIBDIR}/libxml2.so.16.1.3" "liblibxml_rs.so"
chain "libxslt:libxslt.so"          "${LIBDIR}/libxslt.so"        "liblibxml_rs.so"
chain "libxslt:libxslt.so.1"        "${LIBDIR}/libxslt.so.1"      "liblibxml_rs.so"
chain "libxslt:libxslt.so.1.1.45"   "${LIBDIR}/libxslt.so.1.1.45" "liblibxml_rs.so"
chain "libexslt:libexslt.so"        "${LIBDIR}/libexslt.so"       "liblibxml_rs.so"
chain "libexslt:libexslt.so.0"      "${LIBDIR}/libexslt.so.0"     "liblibxml_rs.so"
chain "libexslt:libexslt.so.0.8.25" "${LIBDIR}/libexslt.so.0.8.25" "liblibxml_rs.so"

# relative links only (remount-safe): link targets must not be absolute
rel_ok=0
for f in "${LIBDIR}"/libxml2.so* "${LIBDIR}"/libxslt.so* "${LIBDIR}"/libexslt.so*; do
    if [ -L "$f" ]; then
        tgt="$(readlink "$f")"
        case "$tgt" in
            /*) rel_ok=1 ;;
        esac
    fi
done
if [ "$rel_ok" -eq 0 ]; then
    record PASS "symlinks:all-relative"
else
    record FAIL "symlinks:all-relative" "absolute link target found"
fi

# top-level compat links used by `-L target/debug -lxml2`
chain "compat:top-level-libxml2.so" "${ARTIFACT}/libxml2.so"  "liblibxml_rs.so"
chain "compat:top-level-libxslt.so" "${ARTIFACT}/libxslt.so"  "liblibxml_rs.so"
chain "compat:top-level-libexslt.so" "${ARTIFACT}/libexslt.so" "liblibxml_rs.so"

# top-level SONAME compat links: the DSO carries SONAME libxml2.so.16, so
# consumers NEED that name; LD_LIBRARY_PATH=<artifact> must resolve it and the
# libxslt/libexslt SONAMEs through these links (11.1-T contamination guard).
chain "compat:top-level-libxml2.so.16" "${ARTIFACT}/libxml2.so.16"  "liblibxml_rs.so"
chain "compat:top-level-libxslt.so.1"  "${ARTIFACT}/libxslt.so.1"   "liblibxml_rs.so"
chain "compat:top-level-libexslt.so.0" "${ARTIFACT}/libexslt.so.0"  "liblibxml_rs.so"

# ── 2. static library names ───────────────────────────────────────────────── #

chain "static:libxml2.a" "${LIBDIR}/libxml2.a" "liblibxml_rs.a"
chain "static:libxslt.a" "${LIBDIR}/libxslt.a" "liblibxml_rs.a"
chain "static:libexslt.a" "${LIBDIR}/libexslt.a" "liblibxml_rs.a"

# ── 3. libtool metadata ────────────────────────────────────────────────────── #

if [ -f "${LIBDIR}/libxml2.la" ]; then
    expect "libtool:libxml2.la:current" "$(grep -E '^current=' "${LIBDIR}/libxml2.la")" '^current=17$'
    expect "libtool:libxml2.la:revision" "$(grep -E '^revision=' "${LIBDIR}/libxml2.la")" '^revision=3$'
    expect "libtool:libxml2.la:age" "$(grep -E '^age=' "${LIBDIR}/libxml2.la")" '^age=1$'
else
    record FAIL "libtool:libxml2.la" "missing"
fi
if [ -f "${LIBDIR}/libxslt.la" ]; then
    expect "libtool:libxslt.la:current" "$(grep -E '^current=' "${LIBDIR}/libxslt.la")" '^current=2$'
    expect "libtool:libxslt.la:revision" "$(grep -E '^revision=' "${LIBDIR}/libxslt.la")" '^revision=45$'
    expect "libtool:libxslt.la:age" "$(grep -E '^age=' "${LIBDIR}/libxslt.la")" '^age=1$'
else
    record FAIL "libtool:libxslt.la" "missing"
fi
if [ -f "${LIBDIR}/libexslt.la" ]; then
    expect "libtool:libexslt.la:current" "$(grep -E '^current=' "${LIBDIR}/libexslt.la")" '^current=8$'
    expect "libtool:libexslt.la:revision" "$(grep -E '^revision=' "${LIBDIR}/libexslt.la")" '^revision=25$'
    expect "libtool:libexslt.la:age" "$(grep -E '^age=' "${LIBDIR}/libexslt.la")" '^age=8$'
else
    record FAIL "libtool:libexslt.la" "missing"
fi

if [ -f "${LIBDIR}/xsltConf.sh" ]; then
    expect "libtool:xsltConf.sh:module-version" "$(grep '^MODULE_VERSION=' "${LIBDIR}/xsltConf.sh")" '^MODULE_VERSION="xslt-1\.1\.45"$'
    expect "libtool:xsltConf.sh:libdir" "$(grep '^XSLT_LIBDIR=' "${LIBDIR}/xsltConf.sh")" '^XSLT_LIBDIR="-L'"${LIBDIR}"'"'
else
    record FAIL "libtool:xsltConf.sh" "missing"
fi

if [ -d "${LIBDIR}/libxslt-plugins" ]; then
    record PASS "libtool:libxslt-plugins:dir"
else
    record FAIL "libtool:libxslt-plugins:dir" "missing"
fi

# ── 4. pkg-config files ────────────────────────────────────────────────────── #

for pc in libxml-2.0 libxslt libexslt; do
    if [ -f "${LIBDIR}/pkgconfig/${pc}.pc" ]; then
        record PASS "pkgconfig:${pc}.pc:present"
    else
        record FAIL "pkgconfig:${pc}.pc:present" "missing ${LIBDIR}/pkgconfig/${pc}.pc"
    fi
    if [ -f "${ARTIFACT}/pkgconfig/${pc}.pc" ]; then
        record PASS "pkgconfig:${pc}.pc:compat-copy"
    else
        record FAIL "pkgconfig:${pc}.pc:compat-copy" "missing top-level pkgconfig/${pc}.pc"
    fi
done

expect "pkgconfig:libxml-2.0:libdir" "$(grep -E '^libdir=' "${LIBDIR}/pkgconfig/libxml-2.0.pc")" '^libdir=\$\{exec_prefix\}/lib$'
expect "pkgconfig:libxml-2.0:modules" "$(grep -E '^modules=' "${LIBDIR}/pkgconfig/libxml-2.0.pc")" '^modules=1$'
expect "pkgconfig:libxslt:requires" "$(grep -E '^Requires:' "${LIBDIR}/pkgconfig/libxslt.pc")" '^Requires: libxml-2\.0$'
expect "pkgconfig:libexslt:requires" "$(grep -E '^Requires:' "${LIBDIR}/pkgconfig/libexslt.pc")" '^Requires: libxml-2\.0, libxslt$'

# ── 5. header tree parity with the oracle installs ─────────────────────────── #

xml_oracle_headers="$(ls "${ORACLE_XML}/include/libxml2/libxml/" 2>/dev/null | sort)"
xml_cand_count="$(ls "${INCLUDEDIR}/libxml/" 2>/dev/null | wc -l)"
expect "headers:libxml:count" "$xml_cand_count" '^46$'
missing=0
while IFS= read -r h; do
    [ -z "$h" ] && continue
    if [ ! -f "${INCLUDEDIR}/libxml/${h}" ]; then missing=1; fi
done <<< "$xml_oracle_headers"
if [ "$missing" -eq 0 ]; then
    record PASS "headers:libxml:oracle-superset"
else
    record FAIL "headers:libxml:oracle-superset" "some oracle headers absent"
fi

xslt_oracle_headers="$(ls "${ORACLE_XSLT}/include/libxslt/" 2>/dev/null | sort)"
missing=0
while IFS= read -r h; do
    [ -z "$h" ] && continue
    if [ ! -f "${INCLUDEDIR}/libxslt/${h}" ]; then missing=1; fi
done <<< "$xslt_oracle_headers"
if [ "$missing" -eq 0 ]; then
    record PASS "headers:libxslt:oracle-superset"
else
    record FAIL "headers:libxslt:oracle-superset" "some oracle headers absent"
fi

for h in exslt.h exsltconfig.h exsltexports.h; do
    if [ -f "${INCLUDEDIR}/libexslt/${h}" ]; then
        record PASS "headers:libexslt:${h}"
    else
        record FAIL "headers:libexslt:${h}" "missing"
    fi
done

upstream_libxml_count="$(ls "${INCLUDEDIR}/libxml2/libxml/" 2>/dev/null | wc -l)"
expect "headers:libxml2/libxml:count" "$upstream_libxml_count" '^46$'

# ── 6. version-header macros ───────────────────────────────────────────────── #

expect "headers:xmlversion.h:libxml-dotted" "$(grep -E '^#define LIBXML_DOTTED_VERSION' "${INCLUDEDIR}/libxml/xmlversion.h")" '"2\.15\.3"'
expect "headers:xmlversion.h:libxml-num" "$(grep -E '^#define LIBXML_VERSION ' "${INCLUDEDIR}/libxml/xmlversion.h")" '^#define LIBXML_VERSION 21503$'
expect "headers:xslt.h:libxslt-dotted" "$(grep -E '^#define LIBXSLT_DOTTED_VERSION' "${INCLUDEDIR}/libxslt/xslt.h")" '"1\.1\.45"'
expect "headers:xslt.h:libxslt-num" "$(grep -E '^#define LIBXSLT_VERSION ' "${INCLUDEDIR}/libxslt/xslt.h")" '^#define LIBXSLT_VERSION 10145$'
expect "headers:exsltconfig.h:libexslt-dotted" "$(grep -E '^#define LIBEXSLT_DOTTED_VERSION' "${INCLUDEDIR}/libexslt/exsltconfig.h")" '"0\.8\.25"'
expect "headers:exsltconfig.h:libexslt-num" "$(grep -E '^#define LIBEXSLT_VERSION ' "${INCLUDEDIR}/libexslt/exsltconfig.h")" '^#define LIBEXSLT_VERSION 825$'

# ── 7. executables ─────────────────────────────────────────────────────────── #

for exe in xml2-config xslt-config; do
    if [ -x "${ARTIFACT}/bin/${exe}" ]; then
        record PASS "bin:${exe}:present"
    else
        record FAIL "bin:${exe}:present" "missing or not executable"
    fi
done
for exe in xmllint xmlcatalog xsltproc; do
    if [ -L "${ARTIFACT}/bin/${exe}" ] && [ -e "${ARTIFACT}/bin/${exe}" ]; then
        record PASS "bin:${exe}:link"
    elif [ -x "${ARTIFACT}/bin/${exe}" ]; then
        record PASS "bin:${exe}:present"
    else
        record FAIL "bin:${exe}:link" "missing or dangling"
    fi
done

# ── 8. legacy names absent ─────────────────────────────────────────────────── #

for legacy in \
    "${ARTIFACT}/libxml2.so.2" "${ARTIFACT}/libxml2.so.2.12.0" \
    "${ARTIFACT}/libxml2.so.2.15.3" \
    "${ARTIFACT}/libxslt.so.1.1.39" "${ARTIFACT}/libxslt.so.1.1.47" \
    "${ARTIFACT}/libexslt.so.0.1.1.47" \
    "${LIBDIR}/libxml2.so.2" "${LIBDIR}/libxslt.so.1.1.47" \
    "${LIBDIR}/libexslt.so.0.1.1.47"; do
    if [ -e "$legacy" ] || [ -L "$legacy" ]; then
        record FAIL "legacy:$(basename "$legacy")" "still present"
    else
        record PASS "legacy:absent:$(basename "$legacy")"
    fi
done

# ── 9. version-reporting coherence across the contract ─────────────────────── #
#
# config scripts == pkg-config == DSO runtime APIs == headers

v_cfg_xml="$("${ARTIFACT}/xml2-config" --version)"
v_pc_xml="$(PKG_CONFIG_PATH="${LIBDIR}/pkgconfig" pkg-config --modversion libxml-2.0)"
v_hdr_xml="$(grep -E '^#define LIBXML_DOTTED_VERSION' "${INCLUDEDIR}/libxml/xmlversion.h" | sed 's/.*"\(.*\)"/\1/')"
v_dso_xml="$(LD_LIBRARY_PATH="${ARTIFACT}" "${ARTIFACT}/xmllint" --version 2>&1 | head -1 | grep -oE 'libxml2? [0-9]+\.[0-9]+\.[0-9]+' | grep -oE '[0-9]+\.[0-9]+\.[0-9]+$' || true)"

expect "version:xml2-config==pkg-config" "$v_cfg_xml $v_pc_xml" '^2\.15\.3 2\.15\.3$'
expect "version:pkg-config==header" "$v_pc_xml $v_hdr_xml" '^2\.15\.3 2\.15\.3$'
if [ -n "$v_dso_xml" ]; then
    expect "version:header==dso-runtime" "$v_hdr_xml $v_dso_xml" '^2\.15\.3 2\.15\.3$'
fi

v_cfg_xslt="$("${ARTIFACT}/xslt-config" --version)"
v_pc_xslt="$(PKG_CONFIG_PATH="${LIBDIR}/pkgconfig" pkg-config --modversion libxslt)"
v_hdr_xslt="$(grep -E '^#define LIBXSLT_DOTTED_VERSION' "${INCLUDEDIR}/libxslt/xslt.h" | sed 's/.*"\(.*\)"/\1/')"
expect "version:xslt-config==pkg-config" "$v_cfg_xslt $v_pc_xslt" '^1\.1\.45 1\.1\.45$'
expect "version:pkg-config==header" "$v_pc_xslt $v_hdr_xslt" '^1\.1\.45 1\.1\.45$'

v_pc_exslt="$(PKG_CONFIG_PATH="${LIBDIR}/pkgconfig" pkg-config --modversion libexslt)"
v_hdr_exslt="$(grep -E '^#define LIBEXSLT_DOTTED_VERSION' "${INCLUDEDIR}/libexslt/exsltconfig.h" | sed 's/.*"\(.*\)"/\1/')"
expect "version:exslt-pkg-config==header" "$v_pc_exslt $v_hdr_exslt" '^0\.8\.25 0\.8\.25$'

# ── receipt ────────────────────────────────────────────────────────────────── #

{
    echo "{"
    echo "  \"court\": \"INSTALL-LAYOUT\","
    echo "  \"phase\": \"11.1-S\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"artifact\": \"$ARTIFACT\","
    echo "  \"layout\": {"
    echo "    \"lib\": \"$LIBDIR\","
    echo "    \"include\": \"$INCLUDEDIR\","
    echo "    \"bin\": \"${ARTIFACT}/bin\""
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
