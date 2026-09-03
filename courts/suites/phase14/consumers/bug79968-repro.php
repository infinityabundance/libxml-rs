<?php
// bug79968 repro (candidate crashes at shutdown after adoptNode of a docless text)
error_reporting(E_ALL);
$cdata = new DOMText;
$cdata->before("string");
$cdata->after("string");
$cdata->replaceWith("string");
echo "after replaceWith\n";
$dom = new DOMDocument();
$dom->adoptNode($cdata);
echo "after adoptNode\n";
var_dump($dom->saveXML($cdata));
echo "done\n";
