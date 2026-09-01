#!/bin/bash
# nokogiri-validate-probe.sh — DTD/document validation error counts.
set -uo pipefail
MODE="${1:?usage: nokogiri-validate-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/validate-probe.rb <<'RUBYEOF'
require "nokogiri"
file = "/src/nokogiri/test/files/staff.xml"
doc = Nokogiri::XML.parse(File.read(file), file, &:dtdload)
list = doc.internal_subset.validate(doc)
puts "internal_subset.validate => #{list.length} errors"
list.take(5).each { |e| puts "  #{e.message.inspect}" }
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/validate-probe.rb 2>&1
echo "rc=$?"
