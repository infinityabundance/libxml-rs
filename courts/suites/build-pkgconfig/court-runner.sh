#!/usr/bin/env bash
#
# court-runner.sh — 11.1-S build/pkg-config drop-in court.
#
# Court family: BUILD-PKGCONFIG
#
# Verifies that a consumer using ONLY standard tooling — `cc` together with
# `xml2-config`, `pkg-config` or `xslt-config` — can compile and execute
# against the candidate drop-in libraries without any Rust-specific
# knowledge. This is the literal 11.1-S requirement:
#
#   cc $(xml2-config --cflags) test.c $(xml2-config --libs)
#   cc $(pkg-config --cflags libxml-2.0) test.c $(pkg-config --libs libxml-2.0)
#
# plus the equivalent libxslt paths. The candidate libraries live in the
# artifact directory; LD_LIBRARY_PATH points the dynamic loader at the
# candidate DSO (the documented runtime requirement for a non-system
# installation — see the INSTALL-LAYOUT court receipt).
#
# Evidence: courts/receipts/phase-11/build-pkgconfig-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/build-pkgconfig-${TS}-receipt.json"
mkdir -p "$RECEIPT_DIR"

results=()
pass=0
fail=0
declare -A FAILED_MSGS

record() {
    # record <status> <label> [msg]
    results+=("{\"item\":\"$2\",\"status\":\"$1\"}")
    if [ "$1" = "PASS" ]; then pass=$((pass+1)); else fail=$((fail+1)); FAILED_MSGS["$2"]="${3:-}"; fi
}

sanitize() {
    echo "$1" | tr -d '"\r' | tr '\n' ' '
}

# runtime environment for candidate executables
export LD_LIBRARY_PATH="${ARTIFACT}:${LD_LIBRARY_PATH:-}"

# The plan's commands rely on shell word-splitting of the config output:
#   cc $(xml2-config --cflags) test.c $(xml2-config --libs)
# compile_and_run replicates that (unquoted substitutions) so multi-flag
# outputs like `-L.../lib -lxml2 -lm` split into separate argv entries.
compile_and_run() {
    # compile_and_run <label> <bin-name> <expect> <compile args...>
    label="$1"; bin="$2"; expect="$3"; shift 3
    exe="/tmp/bpc-${bin}"
    rm -f "$exe"
    # shellcheck disable=SC2086 — deliberate word splitting of flag strings
    if ! cc $* -o "$exe" >/tmp/bpc-cc.txt 2>&1; then
        record FAIL "$label" "compile: $(sanitize "$(head -3 /tmp/bpc-cc.txt)")"
        return
    fi
    if [ ! -x "$exe" ]; then
        record FAIL "$label" "no executable produced"
        return
    fi
    if ! "$exe" >/tmp/bpc-run.txt 2>&1; then
        record FAIL "$label" "runtime: $(sanitize "$(head -3 /tmp/bpc-run.txt)")"
        return
    fi
    if ! grep -q "$expect" /tmp/bpc-run.txt; then
        record FAIL "$label" "output missing '$expect': $(sanitize "$(cat /tmp/bpc-run.txt)")"
        return
    fi
    record PASS "$label"
}

XML2_CONFIG="${ARTIFACT}/xml2-config"
XSLT_CONFIG="${ARTIFACT}/xslt-config"
PKG_CONFIG_PATH="${ARTIFACT}/lib/pkgconfig"

# The four required standard-toolchain paths ---------------------------------- #

# 1. xml2-config (libxml2)
# Note: the runtime version string is oracle-matched (the reference system's
# upstream build reports 21503-GITv2.15.3; probe: /usr/lib/libxml2.so.16).
compile_and_run "cc-xml2-config" "libxml-cfg" "version=21503-GITv2.15.3" \
    $( "$XML2_CONFIG" --cflags ) \
    "${SCRIPT_DIR}/test-libxml.c" \
    $( "$XML2_CONFIG" --libs )

# 2. pkg-config libxml-2.0
compile_and_run "cc-pkgconfig-libxml" "libxml-pc" "version=21503-GITv2.15.3" \
    $( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --cflags libxml-2.0 ) \
    "${SCRIPT_DIR}/test-libxml.c" \
    $( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --libs libxml-2.0 )

# 3. xslt-config (libxslt; also exercises the libxml2 symbols it needs)
compile_and_run "cc-xslt-config" "libxslt-cfg" "result=<out>hi</out>" \
    $( "$XSLT_CONFIG" --cflags ) \
    "${SCRIPT_DIR}/test-libxslt.c" \
    $( "$XSLT_CONFIG" --libs )

# 4. pkg-config libxslt (Requires: libxml-2.0 resolves automatically)
compile_and_run "cc-pkgconfig-libxslt" "libxslt-pc" "result=<out>hi</out>" \
    $( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --cflags libxslt ) \
    "${SCRIPT_DIR}/test-libxslt.c" \
    $( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --libs libxslt )

# pkg-config metadata sanity -------------------------------------------------- #

check_pc() {
    # check_pc <module> <expected-version>
    mod="$1"; want="$2"
    v="$( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --modversion "$mod" 2>/dev/null )"
    if [ "$v" = "$want" ]; then
        record PASS "pkg-config:$mod:modversion=$v"
    else
        record FAIL "pkg-config:$mod:modversion" "got '$v', want '$want'"
    fi
    if PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --exists "$mod" 2>/dev/null; then
        record PASS "pkg-config:$mod:exists"
    else
        record FAIL "pkg-config:$mod:exists" "module '$mod' not found"
    fi
}

check_pc libxml-2.0 "2.15.3"
check_pc libxslt "1.1.45"
check_pc libexslt "0.8.25"

# libxslt cflags must carry the libxml2 include resolution (Requires) and the
# exslt module must require both xml2 and xslt.
xslt_cflags="$( PKG_CONFIG_PATH="${PKG_CONFIG_PATH}" pkg-config --cflags libxslt )"
case "$xslt_cflags" in
    *"-I${ARTIFACT}/include"*) record PASS "pkg-config:libxslt:cflags-include" ;;
    *) record FAIL "pkg-config:libxslt:cflags-include" "got: $xslt_cflags" ;;
esac

# receipt --------------------------------------------------------------------- #
{
    echo "{"
    echo "  \"court\": \"BUILD-PKGCONFIG\","
    echo "  \"phase\": \"11.1-S\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"artifact\": \"$ARTIFACT\","
    echo "  \"compiler\": \"$(cc --version | head -1)\","
    echo "  \"pkg_config\": \"$(pkg-config --version)\","
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
