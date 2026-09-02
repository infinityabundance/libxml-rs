#!/bin/bash
# php-run.sh — Phase 14.3 PHP court canonical in-container runner.
#
# Usage: php-run.sh <oracle|candidate>
# Builds the pinned PHP 8.5.10 with the XML-facing extensions (dom, simplexml,
# xml, xmlreader, xmlwriter, xsl) against the selected libxml2 via pkg-config,
# runs the extension test suites, writes:
#   /out/<mode>-configure.log /out/<mode>-make.log /out/<mode>-libver.txt
#   /out/<mode>-full.log /out/<mode>-result.json /out/<mode>-result.md
#
# Configure argv is sourced from php-court-spec.sh (single source of truth);
# PHP 8.5 uses --with-xsl (NOT the stale --enable-xsl).
set -uo pipefail
MODE="${1:?usage: php-run.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"
source /court/consumers/php-court-spec.sh

export MODE="$MODE" CPUS="$(nproc)"

# fresh pristine extraction + single-source-of-truth configure/build
php_prepare_and_build /src/php-build "/out/${MODE}-configure.log" "/out/${MODE}-make.log"
CONF_RC=$?
if [ "$CONF_RC" -ne 0 ]; then exit "$CONF_RC"; fi

cd /src/php-build/php-src

# libxml version actually used
sapi/cli/php -r 'echo "LIBXML_DOTTED_VERSION=", LIBXML_DOTTED_VERSION, "\n";' \
    > "/out/${MODE}-libver.txt" 2>&1 || true
ldd sapi/cli/php 2>/dev/null | grep -E "libxml2|libxslt|libexslt" \
    | sed 's/^[[:space:]]*//;s/=>.*//' > "/out/${MODE}-ldd.txt" 2>&1 || true

# extension suites
LOG="/out/${MODE}-full.log"
make test TESTS="$PHP_TESTS_SPEC" NO_INTERACTION=1 REPORT_EXIT_STATUS=1 \
    > "$LOG" 2>&1
TEST_RC=$?

# authoritative machine-readable + textual result (identical object)
php_tree_env="$PWD"
LOG="$LOG" COURT_OUT="/out/${MODE}-result" PHP_TREE="$php_tree_env" \
  EXT_DIRS="${PHP_EXT_DIRS[*]}" \
  PHP_VERSION="$PHP_VERSION" PHP_SHA256="$PHP_SHA256" \
  CONFIGURE_ARGV="$(php_configure_cmd | tr '\n' ' ')" \
  python3 /court/consumers/php-court-result.py > "/out/${MODE}-result-inline.json" 2>"/out/${MODE}-result-inline.err"
RES_RC=$?
echo "php-run ${MODE} done (test_rc=$TEST_RC result_rc=$RES_RC)"
exit "$RES_RC"
