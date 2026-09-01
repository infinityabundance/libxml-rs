#!/bin/bash
# nokogiri-dtdcontent-probe.sh — inspect internal/external subset contents.
set -uo pipefail
MODE="${1:?usage: nokogiri-dtdcontent-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/dtdcontent-probe.rb <<'RUBYEOF'
require "nokogiri"
file = "/src/nokogiri/test/files/staff.xml"
doc = Nokogiri::XML.parse(File.read(file), file, &:dtdload)
is = doc.internal_subset
es = doc.external_subset
puts "internal_subset=#{is.nil? ? 'nil' : is.name.inspect} external_subset=#{es.nil? ? 'nil' : es.name.inspect}"
if is
  puts "  internal elements: #{is.elements.keys.inspect}"
  puts "  internal attributes: #{is.attributes.keys.inspect}"
end
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/dtdcontent-probe.rb 2>&1
echo "rc=$?"
