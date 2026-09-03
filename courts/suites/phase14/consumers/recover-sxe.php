<?php
error_reporting(E_ALL);
function run($label, $fn) {
    echo "===== $label =====\n";
    try { $r = $fn(); var_dump($r); } catch (\Throwable $e) { echo "THROW " . $e->getMessage() . "\n"; }
}
run("simplexml recover", fn() => simplexml_load_string('<root><child/>', options: LIBXML_RECOVER));
run("simplexml no-recover", fn() => simplexml_load_string('<root><child/>'));
run("simplexml int-errors recover", function () {
    libxml_use_internal_errors(true);
    $r = simplexml_load_string('<root><child/>', options: LIBXML_RECOVER);
    foreach (libxml_get_errors() as $e) {
        echo "ERR: code={$e->code} level={$e->level} msg=" . trim($e->message) . "\n";
    }
    libxml_clear_errors();
    libxml_use_internal_errors(false);
    return $r;
});
echo "done\n";
