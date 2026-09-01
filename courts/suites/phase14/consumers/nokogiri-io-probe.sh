#!/bin/bash
# nokogiri-io-probe.sh — HTML read_io (StringIO) leak/double-free reproduction.
set -uo pipefail
source /court/consumers/lib.sh candidate
cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1
cat > /out/iop.rb <<'RUBYEOF'
require "nokogiri"
require "stringio"
5.times do |i|
  doc = Nokogiri.parse(StringIO.new("<html><head><title></title><body></body></html>"))
  puts "iter#{i}: #{doc.class}"
  GC.start
end
puts "DONE"
RUBYEOF
echo "==== candidate ===="
timeout 60 ruby -rset -Ilib /out/iop.rb 2>&1
echo "rc=$?"
