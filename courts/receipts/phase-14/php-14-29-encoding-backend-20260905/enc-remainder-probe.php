<?php
// R-000157 remainder probe: writer output transcoding for each still-missing
// encoding, candidate vs oracle. Content per encoding is chosen so most
// characters are representable (the unmappable ones exercise the charref
// path). Output must be byte-identical between the two runs.
error_reporting(E_ALL);

function go($enc, $content) {
    $h = fopen("php://output", "w");
    $w = XMLWriter::toStream($h);
    $ok = $w->startDocument(encoding: $enc);
    if (!$ok) { echo "[$enc UNSUPPORTED]\n"; return; }
    $w->writeComment($content);
    $w->startElement("r");
    $w->text($content);
    $w->endElement();
    $w->endDocument();
    unset($w);
}

$cases = [
    "UCS-4LE"      => "A\u{3042}\u{4E2D}",
    "UCS-4BE"      => "A\u{3042}\u{4E2D}",
    "UCS-2"        => "A\u{3042}\u{4E2D}",
    "EBCDIC-US"    => "A B C 123",
    "IBM037"       => "A B C 123",
    "ISO-8859-2"   => "Aąćęłńóśźż",
    "ISO-8859-3"   => "AĞİŞĠ",
    "ISO-8859-4"   => "Aāčēģīķļņ",
    "ISO-8859-5"   => "AБВГДЕЖ",
    "ISO-8859-6"   => "Aةتثجحخد",
    "ISO-8859-7"   => "Aαβγδεζη",
    "ISO-8859-8"   => "Aאבגדהוז",
    "ISO-8859-9"   => "AĞİŞığ",
    "ISO-8859-10"  => "Aāčēģīķļņ",
    "ISO-8859-11"  => "Aกขคงจฉช",
    "ISO-8859-13"  => "Aąčęėįšųū",
    "ISO-8859-14"  => "AŵŷŶ",
    "ISO-8859-15"  => "A€ŠšŽžŒœŸ",
    "ISO-8859-16"  => "Aăâđęîșț",
    "ISO-2022-JP"  => "A\u{3042}\u{4E2D}B",
];
foreach ($cases as $enc => $content) {
    go($enc, $content);
}
