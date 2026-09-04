<?php
$xml = "<allusers><user><uid>bob</uid></user><user><uid>joe</uid></user></allusers>";
$xsl = <<<EOB
<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:php="http://php.net/xsl">
 <xsl:template match="allusers"><x><xsl:for-each select="user"><br/></xsl:for-each></x></xsl:template>
</xsl:stylesheet>
EOB;
$p=new XSLTProcessor(); $p->registerPHPFunctions();
$d=new DOMDocument(); $d->loadXML($xsl); $p->importStyleSheet($d);
$s=new DOMDocument(); $s->loadXML($xml);
echo $p->transformToXML($s);
echo "\n=== fragment case (no wrapper) ===\n";
$xsl2 = <<<EOB
<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:php="http://php.net/xsl">
 <xsl:template match="allusers"><xsl:for-each select="user"><br/></xsl:for-each></xsl:template>
</xsl:stylesheet>
EOB;
$d2=new DOMDocument(); $d2->loadXML($xsl2); $p->importStyleSheet($d2);
$s2=new DOMDocument(); $s2->loadXML($xml);
echo $p->transformToXML($s2);
echo "\n=== nested template LRE inside applied template ===\n";
$xsl3 = <<<EOB
<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:php="http://php.net/xsl">
 <xsl:template match="allusers"><xsl:apply-templates/></xsl:template>
 <xsl:template match="user"><u><br/></u></xsl:template>
</xsl:stylesheet>
EOB;
$d3=new DOMDocument(); $d3->loadXML($xsl3); $p->importStyleSheet($d3);
$s3=new DOMDocument(); $s3->loadXML($xml);
echo $p->transformToXML($s3);
