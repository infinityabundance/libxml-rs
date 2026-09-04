<?php
function run($xsl, $depth, $vars) {
    $xslDom = new DOMDocument();
    $xslDom->loadXML($xsl);
    $doc = new DOMDocument();
    $proc = new XSLTProcessor;
    $proc->maxTemplateDepth = $depth;
    $proc->maxTemplateVars = $vars;
    $proc->importStyleSheet($xslDom);
    $proc->transformToDoc($doc);
    echo "---- transform done ----\n";
}
$recA = '<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template match="/"><xsl:call-template name="recurse"/></xsl:template>
  <xsl:template name="recurse"><xsl:call-template name="recurse"/></xsl:template>
</xsl:stylesheet>';
$recB = '<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template match="/"><xsl:call-template name="recurse"/></xsl:template>
  <xsl:template name="recurse">
    <xsl:param name="COUNT">1</xsl:param>
    <xsl:call-template name="recurse">
      <xsl:with-param name="COUNT" select="$COUNT + 1"/>
    </xsl:call-template>
  </xsl:template>
</xsl:stylesheet>';
echo "===== VARIANT A ====\n";
run($recA, 2, 1000);
echo "===== VARIANT B ====\n";
run($recB, 1<<30, 2);
