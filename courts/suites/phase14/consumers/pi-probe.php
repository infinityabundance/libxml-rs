<?php
foreach (array(
  '<?xml?>',          // position 0, no blank after xml
  '<?xml>',           // position 0, no blank after xml
  ' <?xml?>',         // leading space => PI position
  '<?XML?>',          // uppercase variant at 0
  '<?xml-stylesheet type="text/xsl" href="x"?><r/>',
  '<?xml version="1.0"?><r/>',   // control: real decl
) as $i => $xml) {
    $x = xml_parser_create();
    $ok = xml_parse($x, $xml, true);
    echo "case $i: ok=" . ($ok ? "true" : "false")
       . " code=" . xml_get_error_code($x)
       . " str=" . xml_error_string(xml_get_error_code($x)) . "\n";
    xml_parser_free($x);
}
