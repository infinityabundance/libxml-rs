<?php
error_reporting(E_ALL);
$base = '/srcz/php-src/ext/xsl/tests/xinclude';
chdir($base);
file_put_contents("$base/data2.xml", '<?xml version="1.0"?><data><content>Direct content</content></data>');
$xml = new DOMDocument; $xml->loadXML('<root/>');
function mk($doX, $expr) {
    $style = '<?xml version="1.0"?>
<xsl:stylesheet xmlns:xsl="http://www.w3.org/1999/XSL/Transform" version="1.0">
    <xsl:template match="/root"><out>' . $expr . '</out></xsl:template>
</xsl:stylesheet>';
    $xsl = new DOMDocument; $xsl->loadXML($style);
    $p = new XSLTProcessor; $p->doXInclude = $doX; $p->importStylesheet($xsl);
    return $p;
}
$cases = [
  "data2.xml relative"          => "document('data2.xml')",
  "data.xml relative"           => "document('data.xml')",
  "xincluded.xml relative"      => "document('xincluded.xml')",
  "data.xml abs path"           => "document('$base/data.xml')",
  "data.xml file:// url"        => "document('file://$base/data.xml')",
  "data.xml no-xi subselect"    => "document('data.xml')/data",
];
foreach ($cases as $label => $expr) {
    echo "--- $label ---\n";
    echo trim(mk(false, '<xsl:copy-of select="' . $expr . '"/>')->transformToXml($xml)), "\n";
}
unlink("$base/data2.xml");
