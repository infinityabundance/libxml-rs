<?php
$xml_reader = \XMLReader::XML('
<sparql xmlns="http://www.w3.org/2005/sparql-results#">
 <results>
  <result><binding xml:id="foo" xmlns:custom="urn:custom" custom:foo="bar" name="s"><uri/></binding></result>
 </results>
</sparql>');

$s = $xml_reader->next("sparql");
printf("next(sparql) => %s nodeType=%d name=%s uri=%s\n", var_export($s, true), $xml_reader->nodeType, $xml_reader->name, var_export($xml_reader->namespaceURI, true));

$s = $xml_reader->read();
printf("read => %s nodeType=%d name=%s\n", var_export($s, true), $xml_reader->nodeType, $xml_reader->name);
$s = $xml_reader->next("results");
printf("next(results) => %s nodeType=%d name=%s\n", var_export($s, true), $xml_reader->nodeType, $xml_reader->name);

while ($xml_reader->read()) {
  printf("  read -> nodeType=%d name=%s\n", $xml_reader->nodeType, $xml_reader->name);
  if ($xml_reader->next("result")) {
    printf("  next(result) OK nodeType=%d name=%s\n", $xml_reader->nodeType, $xml_reader->name);
    $result_as_dom_node = $xml_reader->expand();
    printf("  expand => %s\n", var_export($result_as_dom_node !== false, true));
    if ($result_as_dom_node) {
      $child = $result_as_dom_node->firstChild;
      printf("  child nodeType=%d name=%s ns=%s\n", $child->nodeType, $child->nodeName, var_export($child->namespaceURI, true));
    }
    break;
  }
}
