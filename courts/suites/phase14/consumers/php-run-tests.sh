#!/bin/bash
# php-run-tests.sh — Phase-14.3: build PHP candidate (once) and run a chosen
# subset of extension tests, capturing the harness .diff artifacts.
#
# Usage: php-run-tests.sh [TESTS]   (default a single DOM probe)
set -uo pipefail
source /court/consumers/lib.sh candidate
source /court/consumers/php-court-spec.sh
TESTS="${1:-ext/dom/tests/DOMDocument_encoding_basic.phpt}"

php_prepare_and_build /src/php-run /out/php-cfg.log /out/php-make.log || exit $?
cd /src/php-run/php-src

# Run selected tests and capture harness output.
make test TESTS="$TESTS" NO_INTERACTION=1 REPORT_EXIT_STATUS=1 > /out/php-tests.log 2>&1
echo "test rc=$?"
# PHP stores per-test fail diffs next to the .phpt
find ext -name '*.diff' 2>/dev/null | while read -r f; do echo "===== DIFF: $f ====="; cat "$f"; done | head -200
