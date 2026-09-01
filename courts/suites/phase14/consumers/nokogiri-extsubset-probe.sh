#!/bin/bash
# nokogiri-extsubset-probe.sh — external-subset create/load behavior.
set -uo pipefail
MODE="${1:?usage: nokogiri-extsubset-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/ext-probe.rb <<'RUBYEOF'
require "nokogiri"
d = Nokogiri::XML::Document.new
x = d.create_external_subset("staff", nil, "staff.dtd")
puts "created=#{x.nil? ? 'nil' : x.class}"
if x
  puts "  name=#{x.name.inspect} sys=#{x.system_id.inspect} extid=#{x.external_id.inspect}"
end
puts "ext_subset=#{d.external_subset.nil? ? 'nil' : d.external_subset.name.inspect}"

# dtdload path
require "fileutils"
file = "/src/nokogiri/test/files/staff.xml"
d2 = Nokogiri::XML.parse(File.read(file), file, &:dtdload)
puts "dtdload ext_subset=#{d2.external_subset.nil? ? 'nil' : d2.external_subset.name.inspect}"
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/ext-probe.rb 2>&1
echo "rc=$?"
