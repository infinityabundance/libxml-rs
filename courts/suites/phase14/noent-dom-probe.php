<?php
$xml = '<?xml version="1.0"?><!DOCTYPE root [<!ENTITY e "ENT">]>'
  . '<root a="x&e;y">x&e;y<inner b="p&e;q"/></root>';
$d = new DOMDocument();
var_dump($d->loadXML($xml, LIBXML_NOENT));
$xp = new DOMXPath($d);
$r = $d->documentElement;
echo "attr-a=" . $r->getAttribute("a") . "\n";
$inner = $xp->query("//inner")->item(0);
echo "attr-b=" . $inner->getAttribute("b") . "\n";
echo "text=" . $r->textContent . "\n";
fwrite(STDERR, "done\n");
