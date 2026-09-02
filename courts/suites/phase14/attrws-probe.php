<?php
function st($xp, $elem, $attribs) {
  echo "<$elem\n";
  if (sizeof($attribs)) {
    foreach ($attribs as $k => $v) {
      echo "  $k=[" . bin2hex($v) . "]\n";
    }
  }
}
function en($xp, $elem) { echo "</$elem>\n"; }
$xp = xml_parser_create();
xml_set_element_handler($xp, "st", "en");
// tab (0x09), LF (0x0A), CRLF (0x0D 0x0A) inside attribute value
$xml = '<root t="a' . "\x09" . 'b" l="a' . "\x0A" . 'b" r="a' . "\x0D\x0A" . 'b"/>';
var_dump(xml_parse($xp, $xml, true));
fwrite(STDERR, "done\n");
