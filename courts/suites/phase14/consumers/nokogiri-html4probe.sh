#!/bin/bash
# nokogiri-html4probe.sh — isolate HTML4 fragment/parse context leak.
set -uo pipefail
source /court/consumers/lib.sh candidate

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/h4.rb <<'RUBYEOF'
require "nokogiri"
def phase(name)
  5.times do
    yield
    GC.start
  end
  puts "#{name}: phase ok"
end
phase("HTML4.fragment") { Nokogiri::HTML4.fragment("<b>bold</b>").children.first }
phase("HTML4.parse") { Nokogiri::HTML4.parse("<html><body>x</body></html>") }
phase("Nokogiri.make") { Nokogiri.make { b("bold tag") } }
phase("Nokogiri auto") { Nokogiri("<b>bold</b>") }
puts "DONE"
RUBYEOF

echo "==== probe ===="
timeout 60 ruby -rset -Ilib /out/h4.rb 2>&1
echo "rc=$?"
