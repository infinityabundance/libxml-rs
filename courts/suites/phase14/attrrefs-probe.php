<?php
function st($xp, $elem, $attribs) {
  echo "<$elem\n";
  if (sizeof($attribs)) {
    foreach ($attribs as $k => $v) {
      echo "  $k=[" . bin2hex($v) . "]=" . $v . "\n";
    }
  }
}
function en($xp, $elem) { echo "</$elem>\n"; }
$xp = xml_parser_create();
xml_set_element_handler($xp, "st", "en");
$xml = '<?xml version="1.0"?><!DOCTYPE root [<!ENTITY e "ENT">]>'
  . '<root a="x&amp;y" b="x&lt;y" c="x&#38;y" d="x&#60;y" f="x&e;y"'
  . ' g="x&#x41;y" h="x&apos;y" i="x&quot;y"/>';
var_dump(xml_parse($xp, $xml, true));
fwrite(STDERR, "done\n");
