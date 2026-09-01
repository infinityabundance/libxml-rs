#!/bin/bash
# php-dom-probe.sh — run a specific PHP DOM operation against candidate, show text.
set -uo pipefail
source /court/consumers/lib.sh candidate
cd /srcb/php-src
cat > /tmp/domprobe.php <<'PHPEOF'
<?php
$doc = new DOMDocument();
$r = $doc->loadXML('<book><title>X</book></books>');
echo "loadXML ret=" . var_export($r, true) . "\n";
echo "wellFormed="; var_dump($doc->documentElement ? "has" : "none");
?>
PHPEOF
LD_LIBRARY_PATH=/candidate/lib /srcb/php-src/sapi/cli/php /tmp/domprobe.php 2>&1
echo "rc=$?"
