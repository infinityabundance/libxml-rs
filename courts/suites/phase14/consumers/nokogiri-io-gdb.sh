#!/bin/bash
# nokogiri-io-gdb.sh — gdb the StringIO parse double-free.
set -uo pipefail
source /court/consumers/lib.sh candidate
cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1
cat > /out/iop.rb <<'RUBYEOF'
require "nokogiri"
require "stringio"
5.times do |i|
  doc = Nokogiri.parse(StringIO.new("<html><head><title></title><body></body></html>"))
  GC.start
end
puts "DONE"
RUBYEOF
timeout 90 gdb -batch -ex 'run' -ex 'frame 9' -ex 'p/x ctxt' -ex 'x/8gx ctxt' -ex 'bt 45' --args ruby3.1 -rset -Ilib /out/iop.rb > /out/cand-io-gdb.log 2>&1
echo "io-gdb done"
