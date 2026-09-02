<?php
// xml004 reproducer: parse the ext/xml/php xmltest.xml with SAX element
// handlers, whole-file vs 4096-chunk, print <tag/> order.
$path = getenv("XMLTEST_PATH");
if (!$path) $path = "/srcb/php-src/ext/xml/tests/xmltest.xml";

function run($chunk) {
  global $path;
  $xp = xml_parser_create();
  xml_parser_set_option($xp, XML_OPTION_CASE_FOLDING, false);
  xml_set_element_handler($xp, function($p,$e)use(&$o){$o[]="<$e";},
    function($p,$e)use(&$o){$o[]="</$e";});
  $fp = fopen($path,"r"); $buf=""; $o=[];
  if ($chunk) {
    while($data=fread($fp,$chunk)) xml_parse($xp,$data,feof($fp));
  } else {
    while(!feof($fp)){ $buf .= fread($fp,65536); }
    xml_parse($xp,$buf,true);
  }
  fclose($fp);
  echo implode(" ",$o),"\n";
}
echo "single:"; run(0);
echo "chunk :"; run(4096);
fwrite(STDERR,"done\n");
