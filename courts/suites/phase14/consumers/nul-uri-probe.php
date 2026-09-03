<?php
// Probe: file:// URI handling + percent-encoded NUL warnings (bug79971_1 family)
$path = __DIR__;
$plain = "$path/nul-probe-target.xml";
file_put_contents($plain, "<root><a>x</a></root>");

function show($label, $v) {
    echo "$label: ";
    var_dump($v);
}

// 1. Plain path load (baseline)
$s1 = simplexml_load_file($plain);
show("plain-path", $s1 instanceof SimpleXMLElement);

// 2. file:// URI load
$uri = "file://$plain";
$s2 = simplexml_load_file($uri);
show("file-uri", $s2 instanceof SimpleXMLElement);

// 3. file:// URI + %00
$s3 = simplexml_load_file("$uri%00foo");
show("file-uri-percent-nul", $s3);

// 4. asXML to a %00 output URI (only when $s2 is an object)
if ($s2 instanceof SimpleXMLElement) {
    $r = $s2->asXML("$uri.out%00foo");
    show("asxml-percent-nul", $r);
}
@unlink($plain);
?>
