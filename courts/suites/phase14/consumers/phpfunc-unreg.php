<?php
$xsl = new DomDocument();
$xsl->loadXML('<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:foo="urn:foo">
<xsl:template match="//a"><xsl:value-of select="foo:var_dump(string(@href))" /></xsl:template>
</xsl:stylesheet>');
$proc = new XSLTProcessor();
$proc->importStylesheet($xsl);
$inputdom = new DomDocument();
$inputdom->loadXML('<a href="https://php.net">hello</a>');
$r = $proc->transformToXml($inputdom);
var_dump($r);
