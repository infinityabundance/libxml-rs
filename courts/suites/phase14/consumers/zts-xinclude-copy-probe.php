<?php
error_reporting(E_ALL);
chdir('/srcz/php-src/ext/xsl/tests/xinclude');
$xml = new DOMDocument; $xml->loadXML('<root/>');
function mk($doX) {
    $xsl = new DOMDocument;
    $xsl->loadXML('<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <out><xsl:copy-of select="document(\'data.xml\')"/></out>
    </xsl:template>
</xsl:stylesheet>');
    $p = new XSLTProcessor; $p->doXInclude = $doX; $p->importStylesheet($xsl);
    return $p;
}
echo "--- doXInclude=false ---\n";
echo mk(false)->transformToXml($xml), "\n";
echo "--- doXInclude=true ---\n";
echo mk(true)->transformToXml($xml), "\n";
