<?php
function cd($xp, $data) { echo "CD[" . bin2hex($data) . "]=" . $data . "\n"; }
function st($xp, $elem, $attribs) { echo "<$elem\n"; }
function en($xp, $elem) { echo "</$elem>\n"; }
$xp = xml_parser_create();
xml_set_character_data_handler($xp, "cd");
xml_set_element_handler($xp, "st", "en");
$xml = '<?xml version="1.0"?><!DOCTYPE root [<!ENTITY e "ENT">]>'
  . '<root a="x&e;y">x&e;y</root>';
var_dump(xml_parse($xp, $xml, true));
fwrite(STDERR, "done\n");
