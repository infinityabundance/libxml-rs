#!/bin/bash
# nokogiri-dtdid-probe.sh — HTML DTD name/external/system id detection.
set -uo pipefail
MODE="${1:?usage: nokogiri-dtdid-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/dtdid.rb <<'RUBYEOF'
require "nokogiri"
[
  ["HTML5", "<!DOCTYPE html>", true, true],
  ["HTML5 legacy", '<!DOCTYPE HTML SYSTEM "about:legacy-compat">', true, true],
  ["HTML4.01 strict", '<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">', true, false],
  ["XHTML", '<?xml version="1.0"?><!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">', true, false],
].each do |name, ds, exp_html, exp_html5|
  d = Nokogiri::XML(ds)
  dtd = d.internal_subset
  if dtd
    puts "#{name}: name=#{dtd.name.inspect} ext=#{dtd.external_id.inspect} sys=#{dtd.system_id.inspect} html=#{dtd.html_dtd?} html5=#{dtd.html5_dtd?} (exp html5=#{exp_html5})"
  else
    puts "#{name}: no internal_subset"
  end
end
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/dtdid.rb 2>&1
echo "rc=$?"
