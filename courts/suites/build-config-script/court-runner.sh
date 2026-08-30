#!/usr/bin/env bash
#
# court-runner.sh — 11.1-S config-script drop-in court.
#
# Court family: BUILD-CONFIG-SCRIPT
#
# Verifies every option of the candidate `xml2-config` and `xslt-config`
# against the upstream (oracle) script contract:
#
#   --prefix, --exec-prefix, --libs (with --dynamic), --cflags, --version,
#   --help, --prefix=DIR / --exec-prefix=DIR overrides, --modules (xml2),
#   --libtool-libs (xml2), --plugins (xslt), plus error paths (no args,
#   unknown option) and the exit-code discipline of the upstream scripts.
#
# Oracle reference: oracle/historical/prefix/libxml2-2.15.0/bin/xml2-config
# and oracle/historical/prefix/libxslt-1.1.42/bin/xslt-config.
#
# Evidence: courts/receipts/phase-11/build-config-script-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT="${RECEIPT_DIR}/build-config-script-${TS}-receipt.json"
mkdir -p "$RECEIPT_DIR"

XML2_CONFIG="${ARTIFACT}/xml2-config"
XSLT_CONFIG="${ARTIFACT}/xslt-config"

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

# expect <label> <actual> <expected-regex>
expect() {
    label="$1"; actual="$2"; want="$3"
    if printf '%s' "$actual" | grep -qE -- "$want"; then
        record PASS "$label"
    else
        record FAIL "$label" "got: $(sanitize "$actual") | want ~ $(sanitize "$want")"
    fi
}

# exit-code expectation: <label> <cmd...> <want-code>
expect_exit() {
    label="$1"; want="$2"; shift 2
    "$@" >/tmp/csc-out.txt 2>&1
    code=$?
    if [ "$code" -eq "$want" ]; then
        record PASS "$label"
    else
        record FAIL "$label" "exit $code, want $want: $(sanitize "$(head -2 /tmp/csc-out.txt)")"
    fi
}

# ── xml2-config ─────────────────────────────────────────────────────────────── #

expect "xml2-config:--version" "$("$XML2_CONFIG" --version)" '^2\.15\.3$'
expect "xml2-config:--prefix" "$("$XML2_CONFIG" --prefix)" "^${ARTIFACT}$"
expect "xml2-config:--exec-prefix" "$("$XML2_CONFIG" --exec-prefix)" "^${ARTIFACT}$"
expect "xml2-config:--cflags" "$("$XML2_CONFIG" --cflags)" "-I${ARTIFACT}/include/libxml2"
expect "xml2-config:--libs" "$( "$XML2_CONFIG" --libs )" "-L${ARTIFACT}/lib -lxml2"
expect "xml2-config:--libs:has-lm" "$( "$XML2_CONFIG" --libs )" "-lm"
expect "xml2-config:--libs --dynamic" "$( "$XML2_CONFIG" --libs --dynamic )" "-L${ARTIFACT}/lib -lxml2"
expect "xml2-config:--modules" "$("$XML2_CONFIG" --modules)" '^1$'
expect "xml2-config:--libtool-libs" "$("$XML2_CONFIG" --libtool-libs)" "${ARTIFACT}/lib/libxml2\.la"
expect "xml2-config:--prefix=DIR" "$( "$XML2_CONFIG" --prefix=/opt/fake --libs )" "-L/opt/fake/lib -lxml2"
expect "xml2-config:--exec-prefix=DIR" "$( "$XML2_CONFIG" --exec-prefix=/opt/fake --libs )" "-L/opt/fake/lib -lxml2"
expect_exit "xml2-config:--help:exit0" 0 "$XML2_CONFIG" --help
expect_exit "xml2-config:noargs:exit1" 1 "$XML2_CONFIG"
expect_exit "xml2-config:unknown:exit1" 1 "$XML2_CONFIG" --bogus

# ── xslt-config ─────────────────────────────────────────────────────────────── #

expect "xslt-config:--version" "$("$XSLT_CONFIG" --version)" '^1\.1\.45$'
expect "xslt-config:--prefix" "$("$XSLT_CONFIG" --prefix)" "^${ARTIFACT}$"
expect "xslt-config:--exec-prefix" "$("$XSLT_CONFIG" --exec-prefix)" "^${ARTIFACT}$"
expect "xslt-config:--cflags" "$("$XSLT_CONFIG" --cflags)" "-I${ARTIFACT}/include"
expect "xslt-config:--libs" "$( "$XSLT_CONFIG" --libs )" "-L${ARTIFACT}/lib -lxslt -lxml2"
expect "xslt-config:--libs:has-lm" "$( "$XSLT_CONFIG" --libs )" "-lm"
expect "xslt-config:--libs --dynamic" "$( "$XSLT_CONFIG" --libs --dynamic )" "-L${ARTIFACT}/lib -lxslt"
expect "xslt-config:--plugins" "$("$XSLT_CONFIG" --plugins)" "${ARTIFACT}/lib/libxslt-plugins"
expect "xslt-config:--prefix=DIR" "$( "$XSLT_CONFIG" --prefix=/opt/fake --libs )" "-L/opt/fake/lib -lxslt"
# Oracle semantics: --exec-prefix=DIR updates only the exec-prefix VALUE
# (libdir is bound at startup), so --libs still uses the default libdir.
expect "xslt-config:--exec-prefix=DIR" "$( "$XSLT_CONFIG" --exec-prefix=/opt/fake --libs )" "-L${ARTIFACT}/lib -lxslt"
expect "xslt-config:--exec-prefix-value" "$( "$XSLT_CONFIG" --exec-prefix=/opt/fake --exec-prefix )" '^/opt/fake$'
expect_exit "xslt-config:--help:exit0" 0 "$XSLT_CONFIG" --help
expect_exit "xslt-config:noargs:exit1" 1 "$XSLT_CONFIG"
expect_exit "xslt-config:unknown:exit0" 0 "$XSLT_CONFIG" --bogus

# ── version-reporting coherence ────────────────────────────────────────────── #
#
# The config scripts, pkg-config metadata and the DSO's own runtime version
# APIs must agree (11.1-S "version reporting" audit item). These values are
# cross-checked against the DSO in the INSTALL-LAYOUT court.

# receipt --------------------------------------------------------------------- #
{
    echo "{"
    echo "  \"court\": \"BUILD-CONFIG-SCRIPT\","
    echo "  \"phase\": \"11.1-S\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"artifact\": \"$ARTIFACT\","
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
