<?php
// Deep-document parity probe (R-000199): candidate must equal the oracle on
// (a) SAX event sequence + event count for depth-5000 (ext/xml, no tree),
// (b) DOM tree serialize at depth 2000 (byte-identical XML), and (c) the
// DOM tree-depth cap behavior at 2048/2049 (same error text + failure).
error_reporting(E_ALL);

function go_sax($d) {
    $s = "<r>" . str_repeat("<a>", $d) . str_repeat("</a>", $d) . "</r>";
    $p = xml_parser_create();
    $state = new stdClass;
    $state->depth = 0;
    $state->max = 0;
    $state->events = 0;
    $state->sig = "";
    $state->inner = $d;
    xml_set_element_handler($p,
        function ($p, $name) use ($state) {
            $state->depth++;
            $state->max = max($state->max, $state->depth);
            $state->events++;
            if ($state->depth === $state->inner) { $state->sig = "INNER:$name"; }
        },
        function ($p, $name) use ($state) {
            $state->depth--;
            $state->events++;
        });
    $ok = xml_parse($p, $s, true);
    echo "sax d=$d ok=", var_export($ok, true),
        " events=$state->events max=$state->max sig=$state->sig\n";
}

function go_dom($d) {
    $s = "<r>" . str_repeat("<a>", $d) . str_repeat("</a>", $d) . "</r>";
    $doc = new DOMDocument;
    $ok = @$doc->loadXML($s, LIBXML_PARSEHUGE | LIBXML_NONET);
    if ($ok) {
        echo "dom d=$d ok=true bytes=", strlen($doc->saveXML()), "\n";
    } else {
        $e = libxml_get_errors();
        echo "dom d=$d ok=false nerr=", count($e),
            " first=", count($e) ? trim($e[0]->message) : "-", "\n";
        libxml_clear_errors();
    }
}

go_sax(5000);
go_dom(2000);
go_dom(2048);
go_dom(2049);
