<?php
$h = fopen("php://output", "w");
$writer = XMLWriter::toStream($h);
$writer->startDocument(encoding: "SHIFT_JIS");
$writer->writeComment("\u{3041}\u{3041}\u{3041}");
unset($writer);
