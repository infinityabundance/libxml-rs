<?php
$xmls = array(
    '<?xml version="1.0"?><element>',
    '<?xml>',
    '<?xml version="dummy">',
    '<?xml?>',
    '<?xml version="1.0"?><elem></element>',
);
foreach ($xmls as $i => $xml) {
    $xml_parser = xml_parser_create();
    if (!xml_parse($xml_parser, $xml, true)) {
        echo "case $i: code=" . xml_get_error_code($xml_parser)
           . " str=" . xml_error_string(xml_get_error_code($xml_parser)) . "\n";
    } else {
        echo "case $i: OK (no error)\n";
    }
}
