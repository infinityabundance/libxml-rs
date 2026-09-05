<?php
$doc = new DOMDocument;
$doc->loadHTML("<p>foo\0bar</p>", LIBXML_NOERROR);
$html = $doc->saveHTML();
echo "STR:";
var_dump($html);
echo "has-p-foo:"; var_dump(strpos($html, '<p>foo</p>') !== false);

file_put_contents('/tmp/80268.html', "<p>foo\0bar</p>");
$doc = new DOMDocument;
$doc->loadHTMLFile('/tmp/80268.html', LIBXML_NOERROR);
$html = $doc->saveHTML();
echo "FILE:";
var_dump($html);
echo "has-p-foo:"; var_dump(strpos($html, '<p>foo</p>') !== false);
