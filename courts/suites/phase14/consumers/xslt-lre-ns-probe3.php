<?php
$xml = "<allusers><user><uid>bob</uid></user></allusers>";
$tpl = function($body) {
    return "<?xml version=\"1.0\"?>
<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\" xmlns:php=\"http://php.net/xsl\">
 <xsl:template match=\"allusers\">$body</xsl:template>
</xsl:stylesheet>";
};
function run($label, $xsl) {
    $p=new XSLTProcessor(); $d=new DOMDocument(); $d->loadXML($xsl); $p->importStyleSheet($d);
    $s=new DOMDocument(); $s->loadXML("<allusers><user><uid>bob</uid></user></allusers>");
    echo "== $label ==\n", $p->transformToXML($s), "\n";
}
run("fe d>br", $tpl("<xsl:for-each select=\"user\"><d><br/></d></xsl:for-each>"));
run("fe a>b no-ns elems", $tpl("<xsl:for-each select=\"user\"><a><b/></a></xsl:for-each>"));
run("if br", $tpl("<xsl:if test=\"true()\"><br/></xsl:if>"));
run("choose br", $tpl("<xsl:choose><xsl:when test=\"true()\"><br/></xsl:when></xsl:choose>"));
run("var then br", $tpl("<xsl:variable name=\"v\" select=\"1\"/><br/>"));
run("call-template then br", $tpl("<xsl:call-template name=\"t\"/><br/><xsl:template name=\"t\"><span/></xsl:template>"));
