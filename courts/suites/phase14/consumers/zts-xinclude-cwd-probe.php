<?php
error_reporting(E_ALL);
$base = '/srcz/php-src/ext/xsl/tests/xinclude';
chdir($base);
echo "after chdir: ", getcwd(), "\n";
$xml = new DOMDocument; $xml->loadXML('<root/>');
echo "cwd before transform: ", getcwd(), "\n";
$out = mk(false);
echo "cwd after mk: ", getcwd(), "\n";
$res = $out->transformToXml($xml);
echo "cwd after transform: ", getcwd(), "\n";
echo "res: ", trim($res), "\n";

function mk($doX) {
    $xsl = new DOMDocument;
    $xsl->loadXML('<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <container><xsl:value-of select="document(\'data.xml\')/data/content"/></container>
    </xsl:template>
</xsl:stylesheet>');
    $p = new XSLTProcessor; $p->doXInclude = $doX; $p->importStylesheet($xsl);
    return $p;
}
