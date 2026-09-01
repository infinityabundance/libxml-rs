#!/bin/bash
# nokogiri-strict-probe.sh — probe strict-parse error raising in isolation.
set -uo pipefail
MODE="${1:?usage: nokogiri-strict-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/strict-probe.rb <<'RUBYEOF'
require "nokogiri"
def t(label)
  yield
  puts "#{label}: NO RAISE"
rescue => e
  puts "#{label}: RAISED #{e.class}: #{e.message[0,60]}"
end

t("basic options:0") { Nokogiri::XML("<foo><bar></foo>", nil, nil, 0) }
t("basic options kw") { Nokogiri::XML("<foo><bar></foo>", options: 0) }
t("strict block") { Nokogiri::XML("<foo><bar></foo>", &:strict) }
t("strict StringIO") { Nokogiri::XML(StringIO.new("<foo><bar></foo>"), &:strict) }

d = Nokogiri::XML("<foo><bar></foo>", options: 0) rescue nil
if d
  puts "errors count: #{d.errors.length}"
  d.errors.each_with_index { |e,i| puts "  err#{i}: #{e.message}" }
end
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/strict-probe.rb 2>&1
echo "rc=$?"
