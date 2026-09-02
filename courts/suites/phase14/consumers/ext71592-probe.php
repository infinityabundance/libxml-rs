<?php
// SP-14.3.1-4 diagnostic probe: external general parsed entity declared in the
// internal subset, referenced from content, external-entity-ref handler FALSE.
// Oracle (expat-compat on libxml2 2.15): handler fires, parse stops,
// xml_get_error_code() === XML_ERROR_EXTERNAL_ENTITY_HANDLING (21).

$xml = <<<XML
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE root [
  <!ENTITY pic PUBLIC "image.gif" "http://example.org/image.gif">
]>
<root>
<p>&pic;</p>
<p></nop>
</root>
XML;

$parser = xml_parser_create_ns('UTF-8');
$calls = 0;
xml_set_external_entity_ref_handler($parser, function ($p, $names, $base, $sys, $pub) use (&$calls) {
    $calls++;
    fprintf(STDERR, "EXT-REF-HANDLER calls=%d base=%s sys=%s pub=%s\n", $calls, $base, $sys, $pub);
    return false;
});
xml_set_element_handler($parser, function ($p, $name, $attrs) {
    fprintf(STDERR, "START %s\n", $name);
}, function ($p, $name) {
    fprintf(STDERR, "END %s\n", $name);
});
$r = xml_parse($parser, $xml);
$code = xml_get_error_code($parser);
$line = xml_get_current_line_number($parser);
fprintf(STDERR, "rc=%s err=%d line=%d handler_calls=%d eq21=%s\n",
    var_export($r, true), $code, $line, $calls,
    var_export($code === XML_ERROR_EXTERNAL_ENTITY_HANDLING, true));
var_dump($code === XML_ERROR_EXTERNAL_ENTITY_HANDLING);
