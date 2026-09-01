#!/bin/bash
# nokogiri-nsaxisgc-gdb.sh — backtrace for the namespace-axis GC panic.
set -uo pipefail
MODE="${1:?usage: nokogiri-nsaxisgc-gdb.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/nsgc.rb <<'RUBYEOF'
require "nokogiri"
10.times do |i|
  doc = Nokogiri::XML(%Q{<xml><foo xmlns='hello'><bar xmlns:foo='world'/></foo></xml>})
  h = doc.collect_namespaces
  doc.xpath("//namespace::*").length
  GC.start
end
puts "DONE"
RUBYEOF

timeout 90 gdb -batch -ex 'run' -ex 'bt 40' --args ruby3.1 -rset -Ilib /out/nsgc.rb > "/out/${MODE}-nsgdb.log" 2>&1
echo "nsaxis-gc-gdb ${MODE} done"
