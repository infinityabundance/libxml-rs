<?php
foreach (["ISO-8859-1", "ISO-8859-2", "ISO-8859-7", "EBCDIC-US", "UCS-4LE", "UCS-2", "ISO-2022-JP"] as $e) {
    $d = new DOMDocument;
    $ok = @$d->load("/tmp/encin/doc-$e.xml", LIBXML_NONET);
    echo "[$e ", var_export($ok, true), "] ", $ok ? $d->documentElement->textContent : "FAIL", "\n";
    libxml_clear_errors();
}
