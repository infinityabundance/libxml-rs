<?php
$base = "/srcb/php-src/ext/xmlreader/tests";
libxml_use_internal_errors(true);
// 1) DOM schemaValidate of bug73053 doc
$dom = new DOMDocument;
$dom->load("$base/bug73053.xml");
$r = $dom->schemaValidate("$base/bug73053.xsd");
printf("DOM bug73053 schemaValidate => %s\n", var_export($r, true));
foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
libxml_clear_errors();
// 2) DOM schemaValidate of foo vs 013.xsd
$dom = new DOMDocument;
$dom->loadXML("<foo/>");
$r = $dom->schemaValidate("$base/013.xsd");
printf("DOM foo-vs-013 schemaValidate => %s\n", var_export($r, true));
foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
