<?php
// How does the oracle treat a decl after BOM / after whitespace?
$bom = "\xEF\xBB\xBF";
foreach (array(
  'bom-decl'    => $bom . '<?xml version="1.0"?><r/>',
  'ws-decl'     => ' <?xml version="1.0"?><r/>',
  'nl-decl'     => "\n<?xml version=\"1.0\"?><r/>",
  'plain'       => '<?xml version="1.0"?><r/>',
) as $k => $xml) {
    $x = xml_parser_create();
    $ok = xml_parse($x, $xml, true);
    echo "$k: ok=" . ($ok ? "true" : "false")
       . " code=" . xml_get_error_code($x)
       . " str=" . xml_error_string(xml_get_error_code($x)) . "\n";
}
