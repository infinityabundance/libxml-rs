#!/bin/bash
# php-enc-gdb.sh — gdb the DOMDocument::$encoding=UTF-16 segfault against candidate.
# Usage (in container): php-enc-gdb.sh
# Expects: /srcb/php-src built linked to /candidate, lib.sh sourced for env.
set -uo pipefail

cat > /tmp/encprobe.php <<'PHPEOF'
<?php
$doc = new DOMDocument();
$doc->loadXML('<doc>X</doc>');
echo "empty read=[{$doc->encoding}]\n";
$doc->encoding = 'ISO-8859-1';
echo "iso=[{$doc->encoding}]\n";
$doc->encoding = 'UTF-8';
echo "utf8=[{$doc->encoding}]\n";
echo "SETTING UTF-16\n";
$doc->encoding = 'UTF-16';
echo "utf16=[{$doc->encoding}]\n";
echo "DONE\n";
PHPEOF

cd /srcb/php-src
echo "--- running under gdb (candidate) ---"
LD_LIBRARY_PATH=/candidate/lib timeout 90 gdb -batch \
    -ex 'handle SIGSEGV stop print nopass' -ex run -ex bt -ex 'x/12i $rip' \
    --args ./sapi/cli/php /tmp/encprobe.php 2>&1 | grep -vE 'auto-load|\.gdbinit|Debugging|thread_db|libthread_db|To enable|add-auto|set auto|line to|info \"|For more|security protection|^GNU gdb|Copyright|License|This GDB|Warning|warning'
echo "gdb rc done"
