#!/bin/bash
# php-run-tests.sh — build PHP candidate (once) and run a subset of ext tests, capturing diffs.
set -uo pipefail
TESTS="${1:-ext/dom/tests/DOMDocument_encoding_basic.phpt}"
source /court/consumers/lib.sh candidate

cd /src
rm -rf php-build && mkdir php-build && tar xf php-8.5.10.tar.gz -C php-build --strip-components=1
cd php-build
./configure --prefix=/usr/local/php --disable-all --enable-cli \
    --enable-dom --enable-simplexml --enable-xml --enable-xmlreader \
    --enable-xmlwriter --enable-xsl --with-libxml > /out/php-cfg.log 2>&1
make -j"$(nproc)" > /out/php-make.log 2>&1
echo "built rc=$?"

# Run selected tests and show the failure diffs
make test TESTS="$TESTS" NO_INTERACTION=1 REPORT_EXIT_STATUS=1 > /out/php-tests.log 2>&1
echo "test rc=$?"
# PHP stores per-test fail diffs next to the .phpt
find ext -name '*.diff' 2>/dev/null | while read f; do echo "===== DIFF: $f ====="; cat "$f"; done | head -150
