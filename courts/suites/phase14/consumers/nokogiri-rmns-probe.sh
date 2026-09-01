#!/bin/bash
# nokogiri-rmns-probe.sh — remove_namespaces! namespace clearing.
set -uo pipefail
MODE="${1:?usage: nokogiri-rmns-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/rmns.rb <<'RUBYEOF'
require "nokogiri"
doc = Nokogiri::XML(%Q{<root xmlns:a="http://a.x/" xmlns:b="http://b.x/"><a:foo>hi</a:foo><container xmlns:c="http://c.x/"><c:foo>hi2</c:foo></container></root>})
puts "before root.namespaces: #{doc.root.namespaces.inspect}"
doc.remove_namespaces!
puts "after root.namespaces: #{doc.root.namespaces.inspect}"
puts "after to_xml: #{doc.root.to_xml.inspect}"
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/rmns.rb 2>&1
echo "rc=$?"
