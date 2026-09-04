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
run("for-each br top", $tpl("<xsl:for-each select=\"user\"><br/></xsl:for-each>"));
run("for-each x:br top", $tpl("<xsl:for-each select=\"user\"><php:x/></xsl:for-each>"));
run("for-each div top nonempty", $tpl("<xsl:for-each select=\"user\"><div>hi</div></xsl:for-each>"));
run("direct br top", $tpl("<br/>"));
run("direct div nonempty", $tpl("<div>hi</div>"));
run("direct php:x", $tpl("<php:x/>"));
run("wrap then for-each", $tpl("<w><xsl:for-each select=\"user\"><br/></xsl:for-each></w>"));
