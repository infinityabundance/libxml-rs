<?php
$reader = XMLReader::fromString("<root/>");
$props = ["attributeCount","baseURI","depth","hasAttributes","hasValue","isDefault","isEmptyElement","localName","name","namespaceURI","nodeType","prefix","value","xmlLang"];
foreach ($props as $p) {
    try {
        $v = $reader->$p;
        printf("%s => %s\n", $p, var_export($v, true));
    } catch (Throwable $e) {
        printf("%s => THROWS %s\n", $p, get_class($e));
    }
}
echo "--- after read ---\n";
$r = $reader->read();
var_dump($r);
foreach ($props as $p) {
    try {
        $v = $reader->$p;
        printf("%s => %s\n", $p, var_export($v, true));
    } catch (Throwable $e) {
        printf("%s => THROWS %s\n", $p, get_class($e));
    }
}
