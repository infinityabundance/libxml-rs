<?php
error_reporting(E_ALL);
$dom = Dom\XMLDocument::createFromString('<root/>');
$xpath = new Dom\XPath($dom);
$xpath->registerPhpFunctionNs('urn:x', 'test', fn() => $dom->createElement('foo'));
$xpath->registerNamespace('x', 'urn:x');
$test = $xpath->query('x:test()');
var_dump($test[0]->nodeName);
echo "done\n";
