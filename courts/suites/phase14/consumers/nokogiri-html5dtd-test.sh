#!/bin/bash
# nokogiri-html5dtd-test.sh — run JUST test_html_dtd under the candidate.
set -uo pipefail
MODE="${1:?usage: nokogiri-html5dtd-test.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

timeout 60 ruby -rset -Ilib:test:.:test -e 'require "minitest/autorun"; require "test/xml/test_dtd.rb"' -- --name test_html_dtd > "/out/${MODE}-html5.log" 2>&1
echo "html5-dtd test ${MODE} rc=$?"
