#!/bin/bash
# nokogiri-xpath-abort.sh — catch the shutdown double-free (SIGABRT) backtrace.
set -uo pipefail
MODE="${1:?usage: nokogiri-xpath-abort.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

timeout 200 gdb -batch \
  -ex 'run' \
  -ex 'bt 45' \
  --args ruby3.1 -rset -Ilib:test:.:test -e 'require "minitest/autorun"; require "test/xml/test_dtd.rb"; require "test/xml/test_document.rb"; require "test/test_nokogiri.rb"' -- --seed 14472 \
  > "/out/${MODE}-abort.log" 2>&1
echo "abort ${MODE} done"
