<?php
$doc = new DOMDocument();
$doc->loadXML('<container><child1 xmlns:x="http://s/"><x:foo/></child1></container>');
$xpath = new DOMXPath($doc);
echo "--- before removal, child1 ns ---\n";
foreach ($xpath->query('/container/child1/namespace::*') as $ns) {
    echo $ns->nodeName, " = ", var_export($ns->nodeValue, true), "\n";
}
$doc->documentElement->firstElementChild->removeAttributeNS('http://s/', 'x');
echo "--- after removal ---\n";
echo $doc->saveXML();
foreach ($xpath->query('/container/child1/namespace::*') as $ns) {
    echo $ns->nodeName, " = ", var_export($ns->nodeValue, true), "\n";
}
