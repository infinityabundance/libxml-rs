#!/bin/bash
# nokogiri-builder-probe.sh — isolate HTML4 builder doc/root state.
set -uo pipefail
MODE="${1:?usage: nokogiri-builder-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/builder-probe.rb <<'RUBYEOF'
require "nokogiri"
def t(label)
  v = yield
  puts "#{label} => #{v.inspect}"
rescue => e
  puts "#{label} => RAISED #{e.class}: #{e.message[0,60]}"
end

t("make { b }") { Nokogiri.make { b("bold tag") } }
t("Nokogiri { b }") { Nokogiri { b("bold tag") } }

# inspect an HTML4 empty doc
doc = Nokogiri::HTML4::Document.new
t("html4 empty doc children") { doc.children.map(&:name) }
t("html4 empty doc root") { doc.root && doc.root.name }
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/builder-probe.rb 2>&1
echo "rc=$?"
