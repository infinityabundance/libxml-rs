<?php
// Valgrind exercise for the SP-14.3.1-7 streaming/incomplete-doc reader path.
// Mirrors fromStream_broken_stream (partial prefix events + deferred EOF
// error, frozen cursor) plus the legit-complete flow and a re-setup flow.

// 1) broken/incomplete stream: prefix events delivered, error deferred to the
//    read past the last event, cursor frozen on the last node.
$h = fopen("php://memory", "w+");
fwrite($h, "<root><!--my comment-->");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
$start = true;
while (@$reader->read()) {
    if ($start) {
        fwrite($h, "<child/></root>");
        fclose($h);
        $start = false;
    }
}
var_dump($reader->depth);
unset($reader);

// 2) complete doc via fromStream: full event walk, then clean EOF.
$h = fopen("php://memory", "w+");
fwrite($h, "<root><!--c--><a x=\"1\"/></root>");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
while ($reader->read()) {
    if ($reader->nodeType == XMLReader::ELEMENT) {
        $reader->getAttribute("x");
    }
}
var_dump($reader->depth);
unset($reader);

// 3) empty stream: read() fails on the first call (Document is empty).
$h = fopen("php://memory", "w+");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
var_dump(@$reader->read());
unset($reader);

echo "done\n";
