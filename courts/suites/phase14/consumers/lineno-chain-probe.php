<?php
$html = str_repeat("\n", 65536) . <<<EOF
<!doctype html>
<body>
    <p>hello</p>
</body>
EOF;
$dom = Dom\HTMLDocument::createFromString($html);
$n = $dom->documentElement;
for ($i = 0; $n && $i < 6; $i++) {
    printf("node=%s name=%s line=%d\n", get_class($n), $n->nodeName, $n->getLineNo());
    $n = ($i % 2 == 0) ? $n->firstChild : $n->nextSibling;
}
