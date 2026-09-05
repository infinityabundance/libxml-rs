<?php
$doc = new DOMDocument();
$doc->loadHTMLFile('/tmp/definitely-missing-file-xyz.html');
var_dump($doc);
