<?php
// Probe: oracle failure/warning semantics for missing files across APIs
$missing = __DIR__ . "/definitely-missing-xyz.xml";

echo "--- simplexml_load_file missing ---\n";
$r = simplexml_load_file($missing);
var_dump($r);

echo "--- DOMDocument::load missing ---\n";
$d = new DOMDocument();
$r = @$d->load($missing);
var_dump($r);

echo "--- XMLReader::open missing ---\n";
$reader = new XMLReader();
$r = @$reader->open($missing);
var_dump($r);

echo "--- simplexml_load_file missing (no @) ---\n";
$r = simplexml_load_file($missing);
var_dump($r);
?>
