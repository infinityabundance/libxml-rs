#!/bin/bash
# nokogiri-nokogirifn-probe.sh — Nokogiri() auto-detect vs XML for doctype-only inputs.
set -uo pipefail
MODE="${1:?usage: nokogiri-nokogirifn-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/nok.rb <<'RUBYEOF'
require "nokogiri"
def probe(label, ds)
  d = Nokogiri(ds)
  x = d.internal_subset
  kind = d.class.name
  if x
    puts "#{label}: kind=#{kind} dtd=#{x.name.inspect} ext=#{x.external_id.inspect} sys=#{x.system_id.inspect} html5=#{x.html5_dtd?}"
  else
    puts "#{label}: kind=#{kind} NO DTD"
  end
end
probe("HTML5 via Nokogiri()", "<!DOCTYPE html>")
probe("HTML5 legacy via Nokogiri()", '<!DOCTYPE HTML SYSTEM "about:legacy-compat">')
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/nok.rb 2>&1
echo "rc=$?"
