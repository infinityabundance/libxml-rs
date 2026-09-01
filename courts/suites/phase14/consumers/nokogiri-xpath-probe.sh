#!/bin/bash
# nokogiri-xpath-memdump.sh — dump the corrupt attribute node raw bytes.
set -uo pipefail
MODE="${1:?usage: nokogiri-xpath-memdump.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

timeout 200 gdb -batch \
  -ex 'run' \
  -ex 'up 3' \
  -ex 'p/x node' \
  -ex 'x/12gx node' \
  -ex 'x/s node' \
  --args ruby3.1 -rset -Ilib:test:.:test -e 'require "minitest/autorun"; require "test/xml/test_dtd.rb"; require "test/xml/test_document.rb"; require "test/test_nokogiri.rb"' -- --seed 14472 \
  > "/out/${MODE}-mem.log" 2>&1
echo "memdump ${MODE} done"
