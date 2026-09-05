<?php
error_reporting(E_ALL);
$r = new XMLReader;
$ok = @$r->open('data://text/plain,<a><b/></a>');
echo "XMLReader data:// open: ", var_export($ok, true), "\n";
if ($ok) { $r->read(); echo "node: ", $r->name, "\n"; }
