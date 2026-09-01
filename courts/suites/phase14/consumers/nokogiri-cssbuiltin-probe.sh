#!/bin/bash
# nokogiri-cssbuiltin-probe.sh — css-class / local-name-is builtins.
set -uo pipefail
MODE="${1:?usage: nokogiri-cssbuiltin-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/css.rb <<'RUBYEOF'
require "nokogiri"
xml = %Q{<document><thing><div class="title">A</div></thing><thing><p class="content">B</p></thing></document>}
document = Nokogiri::XML(xml)
def t(l); puts "#{l} => #{yield.inspect}"; rescue => e; puts "#{l} => RAISED #{e.class}: #{e.message[0,60]}"; end
t("css .title+.content+p") { document.css(".title", ".content", "p").length }
t("search .//div p.blah") { document.search(".//div", "p.blah").length }
t("xpath .//div,.//p") { document.xpath(".//div", ".//p").length }
t("css p") { document.css("p").length }
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/css.rb 2>&1
echo "rc=$?"
