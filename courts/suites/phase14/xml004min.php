<?php
function st($xp, $elem, $attribs) { echo "<$elem"; if (sizeof($attribs)) { foreach ($attribs as $k => $v) echo " $k=\"$v\""; } echo ">\n"; }
function en($xp, $elem) { echo "</$elem>\n"; }
$xp = xml_parser_create();
xml_set_element_handler($xp, "st", "en");
$xml = '<root id="elem1"><elem1><elem2/></elem1></root>';
xml_parse($xp, $xml, true);
fwrite(STDERR, "end\n");
