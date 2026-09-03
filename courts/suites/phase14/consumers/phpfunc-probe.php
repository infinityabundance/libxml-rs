<?php
$dom = new domDocument();
$dom->load('/srcb/php-src/ext/xsl/tests/xslt011.xsl');
$proc = new xsltprocessor;
$proc->importStylesheet($dom);
$xml = new DomDocument();
$xml->load('/srcb/php-src/ext/xsl/tests/xslt011.xml');
$proc->registerPHPFunctions();
echo $proc->transformToXml($xml);
function foobar($id, $secondArg = "") { return $id . " - " . $secondArg; }
function nodeSet($id = null) {
    if ($id and is_array($id)) { return $id[0]; }
    $dom = new domdocument;
    $dom->loadXML("<root>this is from an external DomDocument</root>");
    return $dom->documentElement;
}
class aClass { static function aStaticFunction($id) { return $id; } }
