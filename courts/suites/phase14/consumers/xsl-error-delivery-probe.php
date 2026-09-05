<?php
// Cross-DSO error delivery probe: an I/O warning raised INSIDE the xsl
// transform (libxslt facade DSO) must surface as a PHP warning (php's error
// handler is registered through the core DSO). If the facade's private error
// slots are consulted instead, the warning prints to stderr instead.
error_reporting(E_ALL);
$xml = new DOMDocument;
$xml->loadXML('<?xml version="1.0"?><root/>');
$xsl = new DOMDocument;
$xsl->loadXML('<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root">
        <out><xsl:value-of select="document(\'definitely-missing-14p27.xml\')/a/b"/></out>
    </xsl:template>
</xsl:stylesheet>');
$proc = new XSLTProcessor;
$proc->importStylesheet($xsl);
ob_start();
$r = $proc->transformToXml($xml);
$captured = ob_get_clean();
echo "transform rc=", var_export($r !== false, true), "\n";
echo "php-handler-captured=", var_export(strlen($captured) > 0, true), "\n";
echo "captured=[", trim($captured), "]\n";
