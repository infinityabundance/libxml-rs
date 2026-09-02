<?php
function cd($xp, $data) { echo "CD[" . bin2hex($data) . "]\n"; }
function st($xp, $elem) { echo "ST<$elem>\n"; }
function en($xp, $elem) { echo "EN</$elem>\n"; }
$cases = array(
  'predef_mid'   => '<root>ab&amp;cd</root>',
  'predef_ahead' => '<root>x&amp;</root>',
  'lt_ahead'     => '<root>x&lt;y</root>',
  'general_mid'  => '<!DOCTYPE root [<!ENTITY e "ENT">]><root>ab&e;cd</root>',
  'general_lead' => '<!DOCTYPE root [<!ENTITY e "ENT">]><root>&e;cd</root>',
  'general_tail' => '<!DOCTYPE root [<!ENTITY e "ENT">]><root>ab&e;</root>',
  'two_generals' => '<!DOCTYPE root [<!ENTITY e "E"><!ENTITY f "F">]><root>&e;&f;</root>',
  'gen_inside'   => '<!DOCTYPE root [<!ENTITY e "E<b>&amp;</b>F">]><root>p&e;q</root>',
);
foreach ($cases as $name => $b) {
  echo "===== $name =====\n";
  $xp = xml_parser_create();
  xml_set_character_data_handler($xp, "cd");
  xml_set_element_handler($xp, "st", "en");
  $r = xml_parse($xp, $b, true);
  echo "parse=" . ($r?1:0) . " err=" . xml_error_string(xml_get_error_code($xp)) . "\n";
  xml_parser_free($xp);
}
fwrite(STDERR, "done\n");
