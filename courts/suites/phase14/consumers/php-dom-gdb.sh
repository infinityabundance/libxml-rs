#!/bin/bash
# php-dom-gdb.sh — run a PHP DOM-style test under gdb for the native backtrace.
set -uo pipefail
TESTF="${1:-ext/dom/tests/DOMDocument_loadHTML_basic.phpt}"
source /court/consumers/lib.sh candidate
source /court/consumers/php-court-spec.sh

php_prepare_and_build /src/php-gdb /out/php-gdb-cfg.log /out/php-gdb-make.log || exit $?

# Extract the .phpt FILE section as a runnable php script
awk 'f{print} /^--FILE--$/{f=1} /^--EXPECT/{exit}' "$TESTF" > /tmp/test.php
cd /src/php-gdb/php-src
echo '--- running under gdb ---'
timeout 60 gdb -batch -ex 'run' -ex 'bt 40' --args ./sapi/cli/php /tmp/test.php > /out/php-gdb.log 2>&1
echo "gdb done"
