<?php
$t = ["&", "a&b", "&a", "a&", "&&", "<", ">", "&quot;", "&amp;", "x", "a&amp;b", ""];
foreach ($t as $s) {
    $xml = "<r>" . htmlspecialchars($s, ENT_XML1 | ENT_NOQUOTES) . "</r>";
    $d = Dom\XMLDocument::createFromString($xml);
    $r = @$d->saveXml();
    if ($r !== false) {
        echo "OK   content=[" . $s . "] xml=[" . $xml . "]\n";
    } else {
        echo "FAIL content=[" . $s . "] xml=[" . $xml . "]\n";
    }
}
