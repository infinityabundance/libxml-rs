<?php
libxml_use_internal_errors(true);
$xsd = '<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t" xmlns:t="urn:t">
  <xs:element name="books">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="book" minOccurs="0"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>';
$f = tempnam(sys_get_temp_dir(), "xsd");
file_put_contents($f, $xsd);
foreach ([
  '<books xmlns="urn:t"><book/></books>',       // children in default ns urn:t
  '<books xmlns="urn:t"><x:book xmlns:x="urn:t"/></books>', // prefixed child
  '<books xmlns="urn:t"><book/></books>' . '',  // (dup)
] as $xml) {
    libxml_clear_errors();
    $dom = new DOMDocument;
    $dom->loadXML($xml);
    $r = $dom->schemaValidate($f);
    printf("xml=%s => %s\n", $xml, var_export($r, true));
    foreach (libxml_get_errors() as $e) printf("  ERR: %s", $e->message);
}
unlink($f);
