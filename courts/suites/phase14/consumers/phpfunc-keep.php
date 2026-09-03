<?php
function makeProcKeep($sel, $keep) {
    $xsl = new DomDocument();
    $xsl->loadXML('<?xml version="1.0" encoding="iso-8859-1" ?>
    <xsl:stylesheet version="1.0"
    xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
    xmlns:php="http://php.net/xsl">
    <xsl:template match="//a"><xsl:value-of select="' . $sel . '" /></xsl:template>
    </xsl:stylesheet>');
    $proc = new XSLTProcessor();
    $proc->importStylesheet($xsl);
    if ($keep) { $GLOBALS['keep'] = $xsl; }
    return $proc;
}
function dumpVal($v) { return "dump: $v"; }
$inputdom = new DomDocument();
$inputdom->loadXML('<?xml version="1.0"?><a href="https://php.net">hello</a>');
for ($keep = 0; $keep <= 1; $keep++) {
    for ($i = 0; $i < 3; $i++) {
        $p = makeProcKeep("php:function('dumpVal', string(@href))", $keep);
        $p->registerPHPFunctions();
        $out = $p->transformToXml($inputdom);
        $good = strpos((string)$out, 'dump: https://php.net') !== false;
        echo "keep=$keep iter=$i " . ($good ? 'OK' : 'BROKEN') . " :: " . substr((string)$out, 0, 70) . "\n";
    }
}
