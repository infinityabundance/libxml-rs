#!/bin/bash
# nokogiri-nsaxis-probe.sh — namespace::* axis output.
set -uo pipefail
MODE="${1:?usage: nokogiri-nsaxis-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/nsaxis.rb <<'RUBYEOF'
require "nokogiri"
doc = Nokogiri::XML(%Q{<xml>\n<foo xmlns='hello'>\n<bar xmlns:foo='world' />\n</foo>\n</xml>\n})
def t(l); puts "#{l} => #{yield.inspect rescue :ERR}"; end
t("collect_namespaces") { doc.collect_namespaces }
t("//namespace::* count") { doc.xpath("//namespace::*").length }
begin
  doc.xpath("//namespace::*").each { |ns| puts "  ns: cls=#{ns.class} prefix=#{ns.respond_to?(:prefix) ? ns.prefix.inspect : 'N/A'} href=#{ns.respond_to?(:href) ? ns.href.inspect : 'N/A'}" }
rescue => e
  puts "  iterate ERR: #{e.class}: #{e.message[0,60]}"
end
t("//b/namespace::* count") { doc.xpath("//b/namespace::*").length }
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/nsaxis.rb 2>&1
echo "rc=$?"
