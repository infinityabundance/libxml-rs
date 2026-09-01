#!/bin/bash
# nokogiri-empty-probe.sh — empty-element serialization oracle-vs-candidate.
set -uo pipefail
MODE="${1:?usage: nokogiri-empty-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/empty-probe.rb <<'RUBYEOF'
require "nokogiri"
def t(label, s)
  d = Nokogiri::XML(s)
  puts "#{label}: root.to_html=#{d.root.to_html.inspect} inner=#{d.inner_html.inspect} xml=#{d.root.to_xml.inspect}"
end
t("a", "<a></a>")
t("b", "<b></b>")
t("br", "<br/>")
t("img", "<img/>")
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/empty-probe.rb 2>&1
echo "rc=$?"
