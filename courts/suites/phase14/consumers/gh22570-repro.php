<?php
ini_set('zend.max_allowed_stack_size', '512K');
$doc = Dom\XMLDocument::createEmpty();
$node = $doc->createElement('a');
for ($i = 0; $i < 100000; $i++) {
    $parent = $doc->createElement('a');
    $parent->appendChild($node);
    $node = $parent;
}
$doc->appendChild($node);
echo "built\n";
try {
    $doc->saveXml();
    echo "saved\n";
} catch (\Error $e) {
    echo "saveXml: ", $e->getMessage(), "\n";
}
echo "done\n";
