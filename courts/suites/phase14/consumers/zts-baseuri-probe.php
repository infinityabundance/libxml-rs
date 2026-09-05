<?php
error_reporting(E_ALL);
$d = '/srcz/php-src/ext/xsl/tests/xinclude';
chdir($d);
$xml = new DOMDocument; $xml->loadXML('<root/>');
$style = '<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <out><xsl:value-of select="base-uri(document(\'data.xml\'))"/></out>
    </xsl:template>
</xsl:stylesheet>';
$xsl = new DOMDocument; $xsl->loadXML($style);
$p = new XSLTProcessor; $p->doXInclude = false; $p->importStylesheet($xsl);
echo "base-uri: ", trim($p->transformToXml($xml)), "\n";
