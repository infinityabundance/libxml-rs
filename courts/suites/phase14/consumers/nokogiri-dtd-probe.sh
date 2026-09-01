#!/bin/bash
# nokogiri-dtd-probe.sh — Phase 14 nokogiri DTD internal_subset/dup double-free probe.
set -uo pipefail
MODE="${1:?usage: nokogiri-dtd-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cat > /out/dtd-probe.rb <<'RUBYEOF'
require "nokogiri"
doc = Nokogiri::XML(<<~XML)
<!DOCTYPE root [
<!ELEMENT root (a)>
<!ELEMENT a (#PCDATA)>
]>
<root><a>x</a></root>
XML
subset = doc.internal_subset
puts "internal_subset: #{subset.name.inspect}"
d2 = doc.dup
puts "dup: #{d2.root.name.inspect}"
2.times { GC.start }
puts "after GC"
RUBYEOF

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

timeout 60 ruby -rset -Ilib /out/dtd-probe.rb > "/out/${MODE}-dtdprobe.log" 2>&1
PROBE_RC=$?
if [ "$MODE" = "candidate" ] && [ "$PROBE_RC" -ne 0 ]; then
    timeout 90 gdb -batch -ex 'run' -ex 'bt 25' \
        --args ruby3.1 -rset -Ilib /out/dtd-probe.rb \
        > "/out/${MODE}-dtdprobe-gdb.log" 2>&1
fi
echo "nokogiri-dtd-probe ${MODE} rc=${PROBE_RC}"
