<?php
$x = new XMLWriter();
$r = $x->openUri('php://memory');
var_dump($r);
if ($r) {
    $x->setIndent(false);
    var_dump($x->startDocument('1.0', 'UTF-8'));
    var_dump($x->startElement('response'));
}
