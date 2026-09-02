<?php
// gh20439-state-probe.php — drive the gh20439_1 per-character feed and, at
// every character, record the parser-visible error state (xml_get_error_code,
// xml_get_current_line_number/byte) so a candidate-vs-oracle differential can
// show whether transient wf/err divergence is observable through the ext/xml
// compat layer. Mirrors gh20439_1 exactly except for the state probes.
$x = xml_parser_create_ns('utf-8');
xml_set_default_handler($x, function( $_parser, $data ) { var_dump($data); });

$input = "<!-- xxx --><foo   attr1='\"&lt;&quot;&#9;&#x0A;&#x0D;&#13;𐍅' attr2=\"&quot;&lt;\"></foo>";
$inputs = str_split($input);
$seen = array();
foreach ($inputs as $ch) {
	xml_parse($x, $ch, false);
	$c = xml_get_error_code($x);
	if ($c !== XML_ERROR_NONE) {
		$seen[$c] = ($seen[$c] ?? 0) + 1;
	}
}
xml_parse($x, "", true);
$c = xml_get_error_code($x);
if ($c !== XML_ERROR_NONE) { $seen[$c] = ($seen[$c] ?? 0) + 1; }
echo "error_codes_seen=" . ($seen ? json_encode($seen) : "none") . "\n";
?>
