<?php
// Depth-cap parity probe: capture the FULL libxml error sequence at the
// 2048 (HUGE) boundary via (a) ext/xml xml_parser and (b) DOM loadXML.
error_reporting(E_ALL);

function go_extxml($d) {
    $s = "<r>" . str_repeat("<a>", $d) . str_repeat("</a>", $d) . "</r>";
    $p = xml_parser_create();
    $ok = xml_parse($p, $s, true);
    $errs = array_map(fn($e) => trim($e->message), libxml_get_errors());
    echo "extxml d=$d ok=", var_export($ok, true), " nerr=", count($errs), "\n";
    foreach ($errs as $m) { echo "   | $m\n"; }
    xml_parser_free($p);
    libxml_clear_errors();
}

function go_dom($d) {
    $s = "<r>" . str_repeat("<a>", $d) . str_repeat("</a>", $d) . "</r>";
    $doc = new DOMDocument;
    $ok = @$doc->loadXML($s, LIBXML_PARSEHUGE | LIBXML_NONET);
    $errs = array_map(fn($e) => trim($e->message), libxml_get_errors());
    echo "dom d=$d ok=", var_export($ok, true), " nerr=", count($errs), "\n";
    foreach ($errs as $m) { echo "   | $m\n"; }
    libxml_clear_errors();
}

go_extxml(2048);
go_extxml(2049);
go_dom(2048);
go_dom(2049);
