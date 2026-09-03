<?php
error_reporting(E_ALL);
function run($label, $fn) {
    echo "===== $label =====\n";
    try {
        $r = $fn();
        var_dump($r);
    } catch (\Throwable $e) {
        echo "THROW " . get_class($e) . ': ' . $e->getMessage() . "\n";
    }
}
run("empty content", function () {
    $s = new SimpleXMLElement('<root/>');
    $s->addChild('a', '');
    return $s->asXML();
});
run("bare amp", function () {
    $s = new SimpleXMLElement('<root/>');
    $s->addChild('a', 'x & y');
    return $s->asXML();
});
run("char ref", function () {
    $s = new SimpleXMLElement('<root/>');
    $s->addChild('a', 'x &#38; y');
    return $s->asXML();
});
run("defined entity", function () {
    $d = new DOMDocument;
    $d->loadXML('<!DOCTYPE r [<!ENTITY e "EE">]><r/>');
    $s = simplexml_import_dom($d);
    $s->addChild('a', 'x &e; y');
    return $s->asXML();
});
echo "done\n";
