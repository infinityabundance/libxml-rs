<?php
libxml_use_internal_errors(true);
$xsd = '<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns:t="urn:t">
  <xs:element name="books" type="xs:string"/>
</xs:schema>';
$f = tempnam(sys_get_temp_dir(), "xsd");
file_put_contents($f, $xsd);

foreach ([
  '<books xmlns="urn:t"/>',
  '<x:books xmlns:x="urn:t"/>',
  '<books/>',
  '<x:books xmlns:x="urn:other"/>',
] as $xml) {
    libxml_clear_errors();
    $dom = new DOMDocument;
    $dom->loadXML($xml);
    $r = $dom->schemaValidate($f);
    printf("xml=%s => %s\n", $xml, var_export($r, true));
    foreach (libxml_get_errors() as $e) printf("  ERR: %s\n", $e->message);
}
unlink($f);
