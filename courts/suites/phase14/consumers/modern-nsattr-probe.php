<?php
$dom = Dom\XMLDocument::createFromString('<test xmlns="http://something/old/"/>');
$t = $dom->documentElement;
echo "ns=", var_export($t->namespaceURI, true), "\n";
echo "attrs:\n";
foreach ($t->attributes as $a) {
    printf("  name=%s prefix=%s local=%s uri=%s value=%s\n",
        $a->nodeName, var_export($a->prefix, true), $a->localName,
        var_export($a->namespaceURI, true), $a->nodeValue);
}
echo "xpath attrs:\n";
$xp = new DOMXPath($dom);
foreach ($xp->query('/test/@*') as $a) {
    printf("  name=%s uri=%s value=%s\n", $a->nodeName, var_export($a->namespaceURI, true), $a->nodeValue);
}
