#!/bin/bash
# nokogiri-ws-probe.sh — characterize leading-whitespace doc children oracle-vs-candidate.
set -uo pipefail
MODE="${1:?usage: nokogiri-ws-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/ws-probe.rb <<'RUBYEOF'
require "nokogiri"
def ws(label, s)
  d = Nokogiri::XML(s)
  kids = d.children.map { |c| "#{c.type}/#{c.name.inspect}/#{c.text.inspect}" }
  puts "#{label}: n=#{d.children.size} kids=#{kids.inspect}"
end

ws("space+pi+root", " <?xml version='1.0' encoding='utf-8' ?><first >")
ws("space+root", "  <root/>")
ws("pi+root", "<?xml version='1.0'?><root/>")
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/ws-probe.rb 2>&1
echo "rc=$?"
