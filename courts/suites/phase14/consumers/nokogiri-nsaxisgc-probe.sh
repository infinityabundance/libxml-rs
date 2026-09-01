#!/bin/bash
# nokogiri-nsaxisgc-probe.sh — namespace-axis results + doc GC churn.
set -uo pipefail
MODE="${1:?usage: nokogiri-nsaxisgc-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/nsgc.rb <<'RUBYEOF'
require "nokogiri"
10.times do |i|
  doc = Nokogiri::XML(%Q{<xml>\n<foo xmlns='hello'>\n<bar xmlns:foo='world' />\n</foo>\n</xml>\n})
  h = doc.collect_namespaces
  n = doc.xpath("//namespace::*").length
  puts "iter#{i}: ns=#{h.inspect} count=#{n}"
  GC.start
end
puts "DONE"
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/nsgc.rb 2>&1
echo "rc=$?"
