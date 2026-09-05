<?php
$doc = new DOMDocument();
$r = $doc->load('/tmp/definitely-missing-file-xyz.xml');
var_dump($r);
$doc2 = new DOMDocument();
$r2 = $doc2->loadHTMLFile('/tmp/definitely-missing-file-xyz.html');
var_dump($r2);
