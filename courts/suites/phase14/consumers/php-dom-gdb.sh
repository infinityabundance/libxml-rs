#!/bin/bash
# php-dom-gdb.sh — run a PHP DOM test under gdb for the segfault backtrace.
set -uo pipefail
TESTF="${1:-ext/dom/tests/DOMDocument_loadHTML_basic.phpt}"
source /court/consumers/lib.sh candidate

cd /src
rm -rf php-build && mkdir php-build && tar xf php-8.5.10.tar.gz -C php-build --strip-components=1
cd php-build
./configure --prefix=/usr/local/php --disable-all --enable-cli \
    --enable-dom --enable-simplexml --enable-xml --enable-xmlreader \
    --enable-xmlwriter --enable-xsl --with-libxml > /out/php-cfg.log 2>&1
make -j"$(nproc)" > /out/php-make.log 2>&1

# Extract the .phpt FILE section as a runnable php script
awk 'f{print} /^--FILE--$/{f=1} /^--EXPECT/{exit}' "$TESTF" > /tmp/test.php
cd /src/php-build
echo '--- running under gdb ---'
timeout 60 gdb -batch -ex 'run' -ex 'bt 40' --args ./sapi/cli/php /tmp/test.php > /out/php-gdb.log 2>&1
echo "gdb done"
