<?php
$xsl = <<<'X'
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
  xmlns:old="http://something/old/">
  <xsl:template match="old:*"><xsl:element name="MARK"/></xsl:template>
  <xsl:template match="*"><xsl:element name="OTHER"/></xsl:template>
</xsl:stylesheet>
X;
$p = new XSLTProcessor();
$sd = new DOMDocument(); $sd->loadXML($xsl); $p->importStylesheet($sd);
$d = new DOMDocument(); $d->loadXML('<old:test xmlns:old="http://something/old/"/>');
echo "prefixed: ", var_export($p->transformToXml($d), true), "\n";
