#!/bin/bash
# nokogiri-misc-probe.sh — isolate misc functional gaps.
set -uo pipefail
MODE="${1:?usage: nokogiri-misc-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/misc-probe.rb <<'RUBYEOF'
require "nokogiri"
require "stringio"

def show(label, val)
  puts "#{label} => #{val.inspect}"
end

# whitespace before prolog
d = Nokogiri::XML(" <?xml version='1.0' encoding='utf-8' ?><first >")
show "lead-space children.size", d.children.size

# encoding
xml = Nokogiri::XML(File.read("/src/nokogiri/test/files/staff.xml"), "/src/nokogiri/test/files/staff.xml", "UTF-8")
show "encoding", xml.encoding

# html empty self-closing
h = Nokogiri::XML("<a></a>")
show "inner_html empty a", h.inner_html

# to_xml indent
doc = Nokogiri::XML("<root><foo><bar/></foo></root>")
out = doc.to_xml(indent: 5)
show "to_xml indent preview", out.lines.map { |l| l.sub(/\s*$/, '').gsub("\t", "<TAB>") }.join("|")

# collect_namespaces
doc2 = Nokogiri::XML(%Q{<xml>\n<foo xmlns='hello'>\n<bar xmlns:foo='world' />\n</foo>\n</xml>\n})
show "collect_namespaces", doc2.collect_namespaces

# namespaces on root
doc3 = Nokogiri::XML(%Q{<root xmlns="http://tenderlovemaking.com/">\n  <foo>\n    bar\n  </foo>\n</root>})
show "root.namespaces", (doc3.root.namespaces if doc3.root)

# xpath xmlns registered
show "xpath xmlns:foo count", doc3.xpath("//xmlns:foo").length
show "css xmlns|foo count", doc3.css("xmlns|foo").length

# remove_namespaces
doc4 = Nokogiri::XML(%Q{<root xmlns:a="http://a.x/" xmlns:b="http://b.x/"><a:foo>hi</a:foo><container xmlns:c="http://c.x/"><c:foo c:attr='v'>hi</c:foo></container></root>})
show "remove_namespaces ns-count before", doc4.root.namespaces.length
doc4.css("foo").each(&:remove)
show "remove_namespaces ns-count after", doc4.root.namespaces.length
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 90 ruby -rset -Ilib /out/misc-probe.rb 2>&1
echo "rc=$?"
