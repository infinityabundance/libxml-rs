#!/bin/bash
# nokogiri-validateall-probe.sh — full error list + count from DTD#validate.
set -uo pipefail
MODE="${1:?usage: nokogiri-validateall-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/valall.rb <<'RUBYEOF'
require "nokogiri"
file = "/src/nokogiri/test/files/staff.xml"
doc = Nokogiri::XML.parse(File.read(file), file, &:dtdload)
list = doc.internal_subset.validate(doc)
puts "count=#{list.length}"
list.each_with_index { |e,i| m=e.message.to_s.sub(/\s+$/,''); m = m[0,10] == "ERROR: " ? m[7..] : m; puts "#{i}: #{m}" }
RUBYEOF

echo "==== #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/valall.rb 2>&1
echo "rc=$?"
