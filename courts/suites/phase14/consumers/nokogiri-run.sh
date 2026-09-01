#!/bin/bash
# nokogiri-run.sh — Phase 14 nokogiri court, in-container runner.
#
# Usage: nokogiri-run.sh <oracle|candidate>
# Builds the pinned nokogiri (git tag v1.19.4) with --use-system-libraries
# against the selected libxml2/libxslt, runs the nokogiri test suite, writes:
#   /out/<mode>-build.log /out/<mode>-libver.txt /out/<mode>-tests.log
#   /out/<mode>-result.json
set -uo pipefail
MODE="${1:?usage: nokogiri-run.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

export NOKOGIRI_USE_SYSTEM_LIBRARIES=yes
cd /src/nokogiri

# ── compile in place (rake-compiler builds the extension into lib/) ───────
bundle install --quiet > "/out/${MODE}-bundle.log" 2>&1 || true
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile > "/out/${MODE}-build.log" 2>&1
BUILD_RC=$?

# libxml2 version actually used
ruby -Ilib -e "require 'nokogiri'; v = Nokogiri::VERSION_INFO; puts({:libxml=>v[:libxml], :libxslt=>v[:libxslt], :other=>v[:other]}.inspect)" \
    > "/out/${MODE}-libver.txt" 2>&1 || true
ldd lib/nokogiri/nokogiri.so 2>/dev/null | grep -E "libxml2|libxslt|libexslt" \
    | sed 's/^[[:space:]]*//;s/=>.*//' > "/out/${MODE}-ldd.txt" || true

if [ "$BUILD_RC" -ne 0 ]; then
    echo "{\"consumer\":\"nokogiri\",\"mode\":\"$MODE\",\"build_ok\":false,\"build_tail\":\"$(tail -c 2000 /out/${MODE}-build.log | tr '\n' ' ' | sed 's/"/\\"/g')\"}" \
        > "/out/${MODE}-result.json"
    exit 0
fi

# ── test suite ─────────────────────────────────────────────────────────────
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake test > "/out/${MODE}-tests.log" 2>&1
TEST_RC=$?

ruby - "$MODE" "$TEST_RC" <<'PYEOF' > "/out/${MODE}-result.json"
require "json"
mode, test_rc = ARGV[0], ARGV[1].to_i
log = File.read("/out/#{mode}-tests.log")
summary = nil
if (m = log.match(/(\d+) runs?, (\d+) assertions?, (\d+) failures?, (\d+) errors?, (\d+) skips?/))
  summary = {runs: m[1].to_i, assertions: m[2].to_i, failures: m[3].to_i,
             errors: m[4].to_i, skips: m[5].to_i}
end
failed_lines = log.lines.grep(/Failure:|Error:|FAIL|ERROR/).first(30)
puts JSON.generate({
  consumer: "nokogiri", mode: mode, build_ok: true, test_rc: test_rc,
  minitest_summary: summary, failed_lines: failed_lines,
  tests_tail: log[-1500..] || "",
})
PYEOF

echo "nokogiri-run ${MODE} done rc=${BUILD_RC}/${TEST_RC}"
