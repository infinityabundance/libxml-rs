<?php
// Phase 14.27 byte-parity probe: XMLWriter output transcoding vs the oracle.
// Run the SAME script against candidate and oracle php; outputs must be
// byte-identical.
error_reporting(E_ALL);

function run($enc, $content) {
    $h = fopen("php://output", "w");
    $w = XMLWriter::toStream($h);
    $w->startDocument(encoding: $enc);
    $w->writeComment($content);           // kana/kanji comment
    $w->startElement("root");
    $w->writeAttribute("a", $content);    // attribute value
    $w->text($content);                   // element text
    $w->endElement();
    $w->endDocument();
    unset($w);
}

// U+3041 hiragana a (SJIS 0x82 0x9F), U+6F22 kanji, half-width katakana U+FF71,
// U+00A5 yen sign, and an unmappable char U+1F600 (emoji, not in SJIS/EUC-JP).
$sjisContent = "\u{3041}\u{6F22}\u{FF71}";
run("SHIFT_JIS", $sjisContent);
$eucContent = "\u{3041}\u{6F22}\u{FF71}";
run("EUC-JP", $eucContent);

// Unmappable-in-legacy chars: oracle iconv EILSEQ -> libxml writes decimal
// charrefs (&#NNNN;). WHATWG/encoding_rs must match byte-for-byte.
$emoji = "\u{1F600}";
run("SHIFT_JIS", $emoji);
