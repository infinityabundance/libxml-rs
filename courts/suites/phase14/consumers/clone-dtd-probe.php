<?php
$xml = <<<'X'
<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><html><body><p>x</p></body></html>
X;
$d = new DOMDocument();
$d->loadXML($xml);
$c = clone $d;
echo "clone ok\n";
echo "save1: "; var_dump(substr($d->saveXML(), 0, 80));
echo "save2: "; var_dump(substr($c->saveXML(), 0, 80));
echo "children: "; var_dump($c->childNodes->length);
$r = $c->documentElement;
echo "root: "; var_dump($r->nodeName);
