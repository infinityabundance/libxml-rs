<?php
$xml = <<<'X'
<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><html><body/></html>
X;
$d = new DOMDocument();
$d->loadXML($xml);
$n = $d->firstChild;
while ($n) { echo get_class($n) . ":" . $n->nodeType . ":" . $n->nodeName . "\n"; $n = $n->nextSibling; }
echo "len=" . $d->childNodes->length . "\n";
