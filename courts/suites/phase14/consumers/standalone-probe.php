<?php
$d = new DOMDocument();
echo "standalone after new: ", var_export($d->xmlStandalone, true), "\n";
$d->loadHTML('<html><body><p>hi</p></body></html>');
echo "standalone after loadHTML: ", var_export($d->xmlStandalone, true), "\n";
echo $d->saveXML(), "---\n";
$d2 = new DOMDocument();
$d2->loadXML('<?xml version="1.0"?><a/>');
echo "standalone after loadXML no-decl: ", var_export($d2->xmlStandalone, true), "\n";
echo $d2->saveXML(), "---\n";
$d3 = new DOMDocument();
$d3->loadHTML('<!DOCTYPE html><html><body>x</body></html>');
echo "standalone after loadHTML2: ", var_export($d3->xmlStandalone, true), "\n";
echo $d3->saveXML(), "---\n";
?>
