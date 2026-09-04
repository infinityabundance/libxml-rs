<?php
error_reporting(E_ALL & ~E_WARNING);
libxml_use_internal_errors(true);
foreach (["<root><!--my comment-->", "<root><a>hi</a><!--c-->"] as $xml) {
    $d = new DOMDocument;
    $ok = $d->loadXML($xml, LIBXML_RECOVER);
    printf("xml=%s ok=%s saveXML=%s\n", $xml, var_export($ok, true), str_replace("\n", "|", $d->saveXML()));
}
