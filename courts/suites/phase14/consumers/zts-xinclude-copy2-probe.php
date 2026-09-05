<?php
error_reporting(E_ALL);
chdir('/srcz/php-src/ext/xsl/tests/xinclude');
$xml = new DOMDocument; $xml->loadXML('<root/>');
function mk($doX, $select) {
    $style = '<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <out>' . $select . '</out>
    </xsl:template>
</xsl:stylesheet>';
    $xsl = new DOMDocument; $xsl->loadXML($style);
    $p = new XSLTProcessor; $p->doXInclude = $doX; $p->importStylesheet($xsl);
    return $p;
}
$copydoc = '<xsl:copy-of select="document(\'data.xml\')"/>';
echo "--- copy-of document(data.xml), doXInclude=false ---\n";
echo mk(false, $copydoc)->transformToXml($xml), "\n";
echo "--- copy-of document(data.xml), doXInclude=true ---\n";
echo mk(true, $copydoc)->transformToXml($xml), "\n";
echo "--- value-of document(data.xml)/data/content, doXInclude=true ---\n";
echo mk(true, '<xsl:value-of select="document(\'data.xml\')/data/content"/>')->transformToXml($xml), "\n";
