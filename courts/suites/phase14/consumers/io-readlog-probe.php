<?php
// Probe: observe exactly when/how libxml pulls bytes from an XMLReader::fromStream
// php stream, and what each read() yields, on a *broken/incomplete* document that
// gets appended-to and closed mid-read (mirrors fromStream_broken_stream.phpt).
class LogMemStream {
    public $context;
    private $inner;
    public static $log = [];
    public static $id = 0;
    public $myid;

    function stream_open($path, $mode, $options, &$opened_path) {
        $this->inner = fopen("php://memory", $mode);
        $this->myid = ++self::$id;
        self::$log[] = ["ev" => "open", "id" => $this->myid];
        return true;
    }
    function stream_read($count) {
        $before = ftell($this->inner);
        $data = fread($this->inner, $count);
        $n = strlen($data);
        self::$log[] = ["ev" => "read", "id" => $this->myid, "count" => $count, "returned" => $n, "pos_before" => $before, "data" => $data];
        return $data;
    }
    function stream_write($data) {
        $pos = ftell($this->inner);
        $n = fwrite($this->inner, $data);
        self::$log[] = ["ev" => "write", "id" => $this->myid, "pos" => $pos, "n" => $n, "data" => $data];
        return $n;
    }
    function stream_tell() { return ftell($this->inner); }
    function stream_seek($offset, $whence) { return fseek($this->inner, $offset, $whence) == 0; }
    function stream_eof() { return feof($this->inner); }
    function stream_stat() { return []; }
    function stream_set_option($o, $a, $b) { return false; }
    function stream_flush() { return fflush($this->inner); }
    function stream_close() {
        self::$log[] = ["ev" => "close", "id" => $this->myid];
        fclose($this->inner);
    }
}
stream_wrapper_register("logmem", LogMemStream::class);

function dump_read($tag, $reader) {
    $nt = $reader->nodeType;
    $extra = "";
    if ($nt == XMLReader::ELEMENT) $extra = " name=" . $reader->name;
    if ($nt == XMLReader::COMMENT) $extra = " value=" . $reader->value;
    if ($nt == XMLReader::TEXT) $extra = " value=" . $reader->value;
    if ($nt == XMLReader::END_ELEMENT) $extra = " name=" . $reader->name;
    printf("%s nodeType=%d depth=%d%s\n", $tag, $nt, $reader->depth, $extra);
}

echo "=== variant 1: exact phpt flow (append + close after first read) ===\n";
LogMemStream::$log = [];
$h = fopen("logmem://x", "w+");
fwrite($h, "<root><!--my comment-->");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
$start = true;
$n = 0;
while ($result = @$reader->read()) {
    $n++;
    dump_read("read#$n result=" . var_export($result, true), $reader);
    if ($start) {
        fwrite($h, "<child/></root>");
        fclose($h);
        $start = false;
    }
}
echo "loop exited after $n reads; depth=" . $reader->depth . "\n";
foreach ([1, 2, 3] as $i) {
    $r = @$reader->read();
    printf("post-loop read#%d result=%s", $i, var_export($r, true));
    dump_read("", $reader);
}
echo "--- stream event log ---\n";
foreach (LogMemStream::$log as $e) {
    $line = json_encode($e);
    if (strlen($line) > 200) $line = substr($line, 0, 200) . "...";
    echo "  $line\n";
}

echo "\n=== variant 2: same but DO NOT close; append only ===\n";
LogMemStream::$log = [];
$h = fopen("logmem://x", "w+");
fwrite($h, "<root><!--my comment-->");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
$start = true;
$n = 0;
while ($result = @$reader->read()) {
    $n++;
    dump_read("read#$n result=" . var_export($result, true), $reader);
    if ($start) {
        fwrite($h, "<child/></root>");
        $start = false;
    }
}
echo "loop exited after $n reads; depth=" . $reader->depth . "\n";
foreach ([1, 2] as $i) {
    $r = @$reader->read();
    printf("post-loop read#%d result=%s", $i, var_export($r, true));
    dump_read("", $reader);
}
echo "--- stream event log ---\n";
foreach (LogMemStream::$log as $e) {
    $line = json_encode($e);
    if (strlen($line) > 200) $line = substr($line, 0, 200) . "...";
    echo "  $line\n";
}

echo "\n=== variant 3: complete doc written before any read (legit usage) ===\n";
LogMemStream::$log = [];
$h = fopen("logmem://x", "w+");
fwrite($h, "<root><!--c--><child/></root>");
fseek($h, 0);
$reader = XMLReader::fromStream($h, encoding: "UTF-8");
$n = 0;
while ($result = @$reader->read()) {
    $n++;
    dump_read("read#$n result=" . var_export($result, true), $reader);
}
echo "loop exited after $n reads; depth=" . $reader->depth . "\n";
echo "--- stream event log ---\n";
foreach (LogMemStream::$log as $e) {
    $line = json_encode($e);
    if (strlen($line) > 200) $line = substr($line, 0, 200) . "...";
    echo "  $line\n";
}
