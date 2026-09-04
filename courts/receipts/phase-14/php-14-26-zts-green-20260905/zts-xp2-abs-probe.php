<?php
error_reporting(E_ALL);
$dir = '/srcz/php-src/ext/xsl/tests/xinclude';
chdir($dir);
echo "php-cwd=", getcwd(), "\n";

$xml = new DOMDocument;
$xml->loadXML('<?xml version="1.0"?><root/>');
function mkxsl($href) {
    $xsl = new DOMDocument;
    $xsl->loadXML('<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root"><container><xsl:value-of select="document(\'' . $href . '\')/data/content"/></container></xsl:template>
</xsl:stylesheet>');
    return $xsl;
}

// (1) document() with ABSOLUTE path + doXInclude=true
$p = new XSLTProcessor; $p->doXInclude = true; $p->importStylesheet(mkxsl($dir . '/data.xml'));
echo "ABS doXInclude: ", trim($p->transformToXml($xml)), "\n";

// (2) document() RELATIVE path, no xinclude, content directly in data2.xml
file_put_contents($dir . '/data2.xml', '<?xml version="1.0"?><data><content>Direct content</content></data>');
$p2 = new XSLTProcessor; $p2->doXInclude = false; $p2->importStylesheet(mkxsl('data2.xml'));
echo "REL plain document(): ", trim($p2->transformToXml($xml)), "\n";

// (3) DOM LIBXML_XINCLUDE relative (core-DSO path)
$d = new DOMDocument;
$ok = @$d->load('data.xml', LIBXML_XINCLUDE);
echo "DOM xinclude rc=", var_export($ok, true), " text=", trim($d->textContent ?? ''), "\n";
unlink($dir . '/data2.xml');
