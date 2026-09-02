<?php
$xml = <<<XML
<?xml version="1.0"?>
<!DOCTYPE root [ <!ENTITY pic PUBLIC "image.gif" "http://example.org/image.gif"> ]>
<root><p>&pic;</p></root>
XML;
$x = xml_parser_create();
xml_set_external_entity_ref_handler($x, function($p,$n,$b,$s,$pi){ echo "EXT: n=$n s=$s pi=$pi\n"; return false; });
$r = xml_parse($x,$xml,true);
echo "r=".($r?"1":"0")." code=".xml_get_error_code($x)." err=".xml_error_string(xml_get_error_code($x))."\n";
$x2 = xml_parser_create_ns('UTF-8');
xml_set_external_entity_ref_handler($x2, function($p,$n,$b,$s,$pi){ echo "EXT2: n=$n\n"; return false; });
$r2 = xml_parse($x2,$xml,true);
echo "r2=".($r2?"1":"0")." code2=".xml_get_error_code($x2)."\n";
fwrite(STDERR,"done\n");
