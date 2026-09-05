<?php
foreach ([2050, 2100, 5000, 20000] as $d) {
    $s = "<r>" . str_repeat("<a>", $d) . str_repeat("</a>", $d) . "</r>";
    $p = xml_parser_create();
    $ok = xml_parse($p, $s, true);
    $e = array_map(fn($x) => trim($x->message), libxml_get_errors());
    echo "d=$d ok=", var_export($ok, true), " errs=[", implode("|", $e), "]\n";
    libxml_clear_errors();
}
