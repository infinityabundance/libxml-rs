#!/bin/bash
# nokogiri-dtdidall-probe.sh — all html_dtd test entries.
set -uo pipefail
MODE="${1:?usage: nokogiri-dtdidall-probe.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/nokogiri
NOKOGIRI_USE_SYSTEM_LIBRARIES=yes rake compile >/dev/null 2>&1

cat > /out/dtdidall.rb <<'RUBYEOF'
require "nokogiri"
data = {
  "HTML 2.0" => ['<!DOCTYPE html PUBLIC "-//IETF//DTD HTML 2.0//EN">', true, false],
  "HTML 3.2" => ['<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">', true, false],
  "XHTML Basic 1.0" => ['<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML Basic 1.0//EN" "http://www.w3.org/TR/xhtml-basic/xhtml-basic10.dtd">', true, false],
  "XHTML 1.0 Strict" => ['<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">', true, false],
  "XHTML host SVG" => ['<!DOCTYPE svg:svg PUBLIC "-//W3C//DTD XHTML 1.1 plus MathML 2.0 plus SVG 1.1//EN" "http://www.w3.org/2002/04/xhtml-math-svg/xhtml-math-svg.dtd">', false, false],
  "CHTML 1.0" => ['<!DOCTYPE HTML PUBLIC "-//W3C//DTD Compact HTML 1.0 Draft//EN">', true, false],
  "HTML 4.01 Strict" => ['<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">', true, false],
  "HTML 4.01 Transitional" => ['<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">', true, false],
  "HTML 4.01 Frameset" => ['<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Frameset//EN" "http://www.w3.org/TR/html4/frameset.dtd">', true, false],
  "HTML 5" => ["<!DOCTYPE html>", true, true],
  "HTML 5 legacy" => ['<!DOCTYPE HTML SYSTEM "about:legacy-compat">', true, true],
  "MathML 2.0" => ['<!DOCTYPE math PUBLIC "-//W3C//DTD MathML 2.0//EN" "http://www.w3.org/Math/DTD/mathml2/mathml2.dtd">', false, false],
}
data.each do |name, (ds, exp_html, exp_html5)|
  d = Nokogiri::XML(ds)
  dtd = d.internal_subset
  if !dtd
    puts "#{name}: NO DTD (exp html5=#{exp_html5})"
  else
    got_html5 = dtd.html5_dtd?
    mark = (got_html5 == exp_html5) ? "OK" : "FAIL"
    puts "#{name}: html5=#{got_html5} [exp #{exp_html5}] #{mark} name=#{dtd.name.inspect} ext=#{dtd.external_id.inspect} sys=#{dtd.system_id.inspect}"
  end
end
RUBYEOF

echo "==== probe: #{MODE} ===="
timeout 60 ruby -rset -Ilib /out/dtdidall.rb 2>&1
echo "rc=$?"
