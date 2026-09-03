<?php
$xml = '<a><b></b></a>';
$o1 = new SimpleXMlElement($xml);
$o2 = clone $o1;
$r = current($o2->xpath('/a'));
echo "r-asXML: ", $r->asXML(), "\n";
$r->addChild('c', new SimpleXMlElement('<c></c>'));
echo "o1: ", $o1->asXML();
echo "o2: ", $o2->asXML();
// indirect: mutate through a fresh xpath on o1 AFTER clone
$o3 = clone $o1;
$r3 = current($o1->xpath('/a'));
echo "r3 from o1: ", $r3->asXML(), "\n";
$r3->addChild('d');
echo "o1-after-d: ", $o1->asXML();
echo "o3: ", $o3->asXML();
?>
