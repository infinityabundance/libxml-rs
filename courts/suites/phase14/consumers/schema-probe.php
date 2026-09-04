<?php
$base = "/srcb/php-src/ext/xmlreader/tests";
libxml_use_internal_errors(true);
foreach (["013.xsd", "bug73053.xsd"] as $xsd) {
    libxml_clear_errors();
    $x = new XMLReader;
    $ok = $x->XML("<items><item>123</item><item>456</item></items>");
    $r = $x->setSchema("$base/$xsd");
    printf("setSchema(%s) => %s\n", $xsd, var_export($r, true));
    foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
}
libxml_clear_errors();
// bug73053 full doc
$x = new XMLReader;
$ok = $x->open("$base/bug73053.xml", null, LIBXML_PARSEHUGE);
$r = $x->setSchema("$base/bug73053.xsd");
printf("bug73053 open=%s setSchema => %s\n", var_export($ok, true), var_export($r, true));
foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
// 013 FAIL doc: read with internal errors
libxml_clear_errors();
$x = new XMLReader;
$x->XML("<foo/>");
$x->setSchema("$base/013.xsd");
while ($x->read() && $x->nodeType != XMLReader::ELEMENT);
printf("after reads, errors:\n");
foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
