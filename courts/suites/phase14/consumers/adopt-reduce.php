<?php
error_reporting(E_ALL);
$case = (int)($argv[1] ?? 0);
switch ($case) {
case 0: // adopt docless text only
    $t = new DOMText;
    $d = new DOMDocument;
    $d->adoptNode($t);
    break;
case 1: // before/after/replaceWith only (no adopt)
    $t = new DOMText;
    $t->before("string");
    $t->after("string");
    $t->replaceWith("string");
    break;
case 2: // adopt + saveXML like the phpt
    $t = new DOMText;
    $d = new DOMDocument;
    $d->adoptNode($t);
    var_dump($d->saveXML($t));
    break;
case 3: // saveXML on non-adopted docless text
    $t = new DOMText;
    $d = new DOMDocument;
    var_dump($d->saveXML($t));
    break;
case 4: // adopt a docless element
    $t = new DOMElement("x");
    $d = new DOMDocument;
    $d->adoptNode($t);
    break;
case 5: // text with a doc from createTextNode then adopt to other doc
    $d1 = new DOMDocument;
    $t = $d1->createTextNode("s");
    $d2 = new DOMDocument;
    $d2->adoptNode($t);
    break;
}
echo "OK case $case\n";
