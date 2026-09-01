#!/bin/bash
# nokogiri-teardown-probe.sh — isolate the teardown double-free.
set -uo pipefail
MODE="${1:?usage: nokogiri-teardown-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/teardown.rb <<'RUBYEOF'
require "nokogiri"
5.times do |i|
  doc = Nokogiri::XML("<root><a>x</a></root>")
  doc.xpath("//a")
  GC.start
end
puts "phase1 done"
5.times do |i|
  d = Nokogiri::XML('<!DOCTYPE root><root><a>x</a></root>')
  is = d.internal_subset
  is.validate(d) if is
  GC.start
end
puts "phase2 done"
puts "DONE"
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/teardown.rb 2>&1
echo "rc=$?"
