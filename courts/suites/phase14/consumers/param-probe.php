<?php
function mk() {
  $xslDom = new DOMDocument();
  $xslDom->loadXML('<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                                xmlns:test="http://www.php.net/test">
    <xsl:param name="foo" select="' . "'" . 'EMPTY' . "'" . '"/>
    <xsl:param name="test:foo" select="' . "'" . 'EMPTY' . "'" . '"/>
    <xsl:template match="/root">
        <out a="{$foo}" b="{$test:foo}"/>
    </xsl:template>
  </xsl:stylesheet>');
  $p = new XSLTProcessor();
  $p->importStyleSheet($xslDom);
  return $p;
}
$xmlDom = new DOMDocument();
$xmlDom->loadXML('<root/>');

$p = mk();
$p->setParameter("http://www.php.net/test", "foo", "SET2");
print "transform1: " . $p->transformToXML($xmlDom) . "\n";

$p = mk();
$p->setParameter("", "foo", "SET1");
print "transform2: " . $p->transformToXML($xmlDom) . "\n";

$p = mk();
$p->setParameter("", "foo", "SET1");
$p->setParameter("http://www.php.net/test", "foo", "SET2");
print "transform3: " . $p->transformToXML($xmlDom) . "\n";
$p->removeParameter("http://www.php.net/test", "foo");
print "transform4 (removed ns foo): " . $p->transformToXML($xmlDom) . "\n";

// namespaced param on a stylesheet WITHOUT declaration of it
$xslDom = new DOMDocument();
$xslDom->loadXML('<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:param name="foo" select="' . "'" . 'EMPTY' . "'" . '"/>
  <xsl:template match="/root"><out a="{$foo}"/></xsl:template>
</xsl:stylesheet>');
$p = new XSLTProcessor();
$p->importStyleSheet($xslDom);
$p->setParameter("http://www.php.net/test", "foo", "SET2");
print "transform5 (undeclared ns): " . $p->transformToXML($xmlDom) . "\n";
