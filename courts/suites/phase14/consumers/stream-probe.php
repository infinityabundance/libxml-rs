<?php
error_reporting(E_ALL);
$h = fopen("php://memory", "w+");
fwrite($h, "<root><!--my comment-->");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
$start = true;
$i = 0;
while (($result = $reader->read()) !== null && $i < 8) {
    $i++;
    printf("read#%d => %s nodeType=%d name=%s\n", $i, var_export($result, true), $reader->nodeType, $reader->name);
    if ($start) {
        fwrite($h, "<child/></root>");
        fclose($h);
        $start = false;
    }
}
printf("depth=%d\n", $reader->depth);
