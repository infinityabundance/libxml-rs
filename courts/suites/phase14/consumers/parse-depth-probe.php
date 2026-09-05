<?php
// Parser nesting-depth envelope probe (R-000199). Parses a doc with
// `$depth` nested <a> elements via DOM (SAX2 tree build). The oracle
// (iterative xmlParseTryOrFinish) parses depth-20000 on a 1MB stack; the
// candidate's recursive descent used to crash around 3-4k at -O0.
error_reporting(E_ALL);
$depth = isset($argv[1]) ? (int)$argv[1] : 5000;
$s = "<r>" . str_repeat("<a>", $depth) . str_repeat("</a>", $depth) . "</r>";
$d = new DOMDocument;
$ok = @$d->loadXML($s, LIBXML_NONET | LIBXML_PARSEHUGE);
if (!$ok) {
    echo "PARSE-FAIL depth=$depth\n";
    foreach (libxml_get_errors() as $e) { echo "  ", trim($e->message), "\n"; }
    exit(1);
}
// Walk to the innermost element to prove the tree really is $depth deep.
$n = $d->documentElement;
$count = 0;
while ($n->firstChild) { $n = $n->firstChild; $count++; }
echo "OK depth=$depth inner=$count root=", $d->documentElement->nodeName, "\n";
