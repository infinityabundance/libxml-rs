<?php
function cd($xp, $data) { echo "CD[" . $data . "]\n"; }
function st($xp, $elem, $attribs) { echo "<$elem\n"; }
function en($xp, $elem) { echo "</$elem>\n"; }
$xp = xml_parser_create();
xml_set_character_data_handler($xp, "cd");
xml_set_element_handler($xp, "st", "en");
$xml = '<?xml version="1.0"?><!DOCTYPE root [<!ENTITY e "ENT">]>'
  . '<root>a&e;b</root>';
$r = xml_parse($xp, $xml, true);
var_dump($r);
if (!$r) {
  echo "err=" . xml_error_string(xml_get_error_code($xp)) . "\n";
  echo "line=" . xml_get_current_line_number($xp) . "\n";
}
fwrite(STDERR, "done\n");
