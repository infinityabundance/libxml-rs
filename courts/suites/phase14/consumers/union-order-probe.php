<?php
$d = new DOMDocument();
$d->loadXML('<root a="1" b="2"><child c="x"/></root>');
$xp = new DOMXPath($d);
foreach ($xp->query('/root/node()|/root/@*') as $n) {
    echo $n->nodeName, "\n";
}
echo "---reverse---\n";
foreach ($xp->query('/root/@*|/root/node()') as $n) {
    echo $n->nodeName, "\n";
}
