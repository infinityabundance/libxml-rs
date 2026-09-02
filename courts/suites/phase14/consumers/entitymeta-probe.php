<?php
$xml = <<<XML
<?xml version="1.0"?>
<!DOCTYPE test [
  <!ENTITY a PUBLIC "public id" "external.stuff" NDATA stuff>
  <!ENTITY b PUBLIC "" "external.stuff" NDATA stuff>
  <!ENTITY c SYSTEM "s.stuff" NDATA n>
  <!ENTITY d SYSTEM "s2.stuff" NDATA >
]>
<root/>
XML;
$d = new DOMDocument(); var_dump($d->loadXML($xml));
foreach (iterator_to_array($d->doctype->entities) as $e) {
  printf("ent=%s etype-raw: pub=%s sys=%s not=%s\n",
    $e->nodeName,
    var_export($e->publicId, true)?: '?',
    $e->systemId===null?'NULL':$e->systemId,
    $e->notationName===null?'NULL':$e->notationName);
  echo "  publicId="; var_dump($e->publicId); echo "  systemId="; var_dump($e->systemId); echo "  notationName="; var_dump($e->notationName);
}
fwrite(STDERR,"done\n");
