<?php
/**
 * php_driver.php — §16.12 consumer driver for PHP's XML stack (PHP 8.5).
 *
 * Invocation (inside the perf container, after the provider has been selected
 * by sourcing /court/consumers/lib.sh):
 *
 *   php php_driver.php --id ID --category CAT --file PATH --reps N \
 *       [--warmup W] [--ops a,b,c]
 *
 * The provider is selected by the caller through LD_LIBRARY_PATH (both the
 * upstream oracle at /usr/local and the libxml-rs candidate at /candidate ship
 * the same DSO SONAMEs — libxml2.so.16, libxslt.so.1, libexslt.so.0 — so one
 * CLI binary links against either provider at run time). This driver never
 * touches the provider environment.
 *
 * stdout: exactly one single-line JSON object, e.g.
 *
 *   {"consumer":"php","id":"svg-001","category":"SVG","file":"/corpus/svg-001.svg",
 *    "ops":{"dom":{"ok":true,"ms":0.41,"fingerprint":"sha256:…","detail":""}, …}}
 *
 * `ms` is the best in-process engine time of --reps timed repetitions after
 * --warmup warmups, measured with the monotonic clock (hrtime) in milliseconds.
 *
 * Fingerprints (see INTERFACE.md §16.14) capture only the observable result and
 * are deliberately free of timestamps, pointers, absolute paths and provider /
 * version strings, so oracle and candidate agree on the same file + op:
 *
 *   dom / simplexml   canonical document-order DOM walk (node kind, element and
 *                     attribute local names, namespace URI, attribute values,
 *                     coalesced text) hashed sha256
 *   xmlreader         ordered start/end element-name + text-length stream
 *   domxpath          per-expression result type + cardinality + items
 *                     (canonical node path for node-sets)
 *   serialize / xsltprocessor   sha256 of the output bytes
 *
 * Every op is fail-soft: a failure yields {"ok":false,"error":"…"} (with ms
 * null / empty fingerprint) and never aborts the process.
 */

ini_set('memory_limit', '-1');   // large corpus members; identical on both providers
ini_set('display_errors', 'stderr');
ini_set('html_errors', '0');
libxml_use_internal_errors(true);

const PB_DEFAULT_OPS = ['dom', 'xmlreader', 'simplexml', 'domxpath', 'xsltprocessor', 'serialize'];

/* ------------------------------------------------------------------ */
/* argument parsing                                                    */
/* ------------------------------------------------------------------ */

$pb_args = [];
$pb_argv = $_SERVER['argv'] ?? [];
$pb_n = count($pb_argv);
for ($i = 1; $i < $pb_n; $i++) {
    $a = $pb_argv[$i];
    if (!is_string($a) || strncmp($a, '--', 2) !== 0) {
        continue;
    }
    $key = substr($a, 2);
    $val = '';
    $eq = strpos($key, '=');
    if ($eq !== false) {
        $val = substr($key, $eq + 1);
        $key = substr($key, 0, $eq);
    } elseif ($i + 1 < $pb_n && (!is_string($pb_argv[$i + 1]) || strncmp($pb_argv[$i + 1], '--', 2) !== 0)) {
        $val = $pb_argv[++$i];
    } else {
        $val = '1';
    }
    $pb_args[$key] = $val;
}

$pb_id       = (string)($pb_args['id'] ?? '');
$pb_category = (string)($pb_args['category'] ?? '');
$pb_file     = (string)($pb_args['file'] ?? '');
$pb_reps     = isset($pb_args['reps']) ? (int)$pb_args['reps'] : 1;
$pb_warmup   = isset($pb_args['warmup']) ? (int)$pb_args['warmup'] : 1;
if ($pb_reps < 1) {
    $pb_reps = 1;
}
if ($pb_warmup < 0) {
    $pb_warmup = 0;
}

$pb_ops = PB_DEFAULT_OPS;
if (isset($pb_args['ops']) && $pb_args['ops'] !== '') {
    $wanted = [];
    foreach (explode(',', $pb_args['ops']) as $o) {
        $o = trim($o);
        if ($o !== '') {
            $wanted[] = $o;
        }
    }
    if ($wanted) {
        $pb_ops = $wanted;
    }
}

/* ------------------------------------------------------------------ */
/* resource locations (/bench per INTERFACE.md; __DIR__ as a fallback) */
/* ------------------------------------------------------------------ */

$pb_bench = getenv('BENCH_DIR');
if (!is_string($pb_bench) || $pb_bench === '') {
    $pb_bench = is_file('/bench/families.json') ? '/bench' : __DIR__;
}

$pb_families = [];
$pb_fam_path = $pb_bench . '/families.json';
if (is_file($pb_fam_path)) {
    $decoded = json_decode((string)file_get_contents($pb_fam_path), true);
    if (is_array($decoded) && isset($decoded['families']) && is_array($decoded['families'])) {
        $pb_families = $decoded['families'];
    }
}

/* ------------------------------------------------------------------ */
/* small helpers                                                       */
/* ------------------------------------------------------------------ */

function pb_ok(float $ms, string $hex, string $detail = ''): array
{
    return ['ok' => true, 'ms' => round($ms, 6), 'fingerprint' => 'sha256:' . $hex, 'detail' => $detail];
}

function pb_fail(string $err): array
{
    return ['ok' => false, 'ms' => null, 'fingerprint' => '', 'detail' => '', 'error' => $err];
}

function pb_err_text(): string
{
    $errs = libxml_get_errors();
    libxml_clear_errors();
    if (!$errs) {
        return 'unknown libxml error';
    }
    $e = $errs[count($errs) - 1];
    return trim((string)$e->message) . ' (line ' . (int)$e->line . ')';
}

/** Length-prefixed, delimiter-framed field: unambiguous within a record. */
function pb_s(string $s): string
{
    return strlen($s) . ':' . $s . "\x1f";
}

/** Canonical numeric rendering (XPath count()/number() results are floats). */
function pb_num(float $v): string
{
    if (is_nan($v)) {
        return 'NaN';
    }
    if (is_infinite($v)) {
        return $v > 0 ? 'Infinity' : '-Infinity';
    }
    if ($v == floor($v) && abs($v) < 1e15) {
        return sprintf('%.0f', $v);
    }
    return rtrim(rtrim(sprintf('%.10f', $v), '0'), '.');
}

/* ------------------------------------------------------------------ */
/* canonical DOM walk (dom / simplexml fingerprints)                   */
/* ------------------------------------------------------------------ */

function pb_walk(DOMNode $n, string &$out): void
{
    $t = $n->nodeType;

    if ($t === XML_ELEMENT_NODE) {
        $out .= "E\x1f" . pb_s((string)($n->localName ?? '')) . pb_s((string)($n->namespaceURI ?? ''));
        if ($n->hasAttributes()) {
            foreach ($n->attributes as $attr) {
                $out .= "A\x1f" . pb_s((string)($attr->localName ?? ''))
                    . pb_s((string)($attr->namespaceURI ?? '')) . pb_s((string)$attr->value);
            }
        }
        $out .= "\x1e";
        // Coalesce runs of adjacent text/CDATA nodes so entity-driven node
        // splitting cannot change the fingerprint.
        $pending = null;
        for ($c = $n->firstChild; $c !== null; $c = $c->nextSibling) {
            $ct = $c->nodeType;
            if ($ct === XML_TEXT_NODE || $ct === XML_CDATA_SECTION_NODE) {
                $pending = ($pending ?? '') . (string)$c->nodeValue;
                continue;
            }
            if ($pending !== null) {
                $out .= "T\x1f" . pb_s($pending) . "\x1e";
                $pending = null;
            }
            pb_walk($c, $out);
        }
        if ($pending !== null) {
            $out .= "T\x1f" . pb_s($pending) . "\x1e";
        }
        $out .= "Z\x1f" . pb_s((string)($n->localName ?? '')) . "\x1e";
        return;
    }

    if ($t === XML_TEXT_NODE || $t === XML_CDATA_SECTION_NODE) {
        $out .= "T\x1f" . pb_s((string)$n->nodeValue) . "\x1e";
        return;
    }
    if ($t === XML_COMMENT_NODE) {
        $out .= "C\x1f" . pb_s((string)$n->nodeValue) . "\x1e";
        return;
    }
    if ($t === XML_PI_NODE) {
        $out .= "P\x1f" . pb_s((string)$n->target) . pb_s((string)$n->data) . "\x1e";
        return;
    }
    if ($t === XML_DOCUMENT_TYPE_NODE) {
        $out .= "DT\x1f" . pb_s((string)$n->name) . "\x1e";
        return;
    }
    if ($t === XML_DOCUMENT_NODE || $t === XML_DOCUMENT_FRAG_NODE || $t === XML_HTML_DOCUMENT_NODE) {
        for ($c = $n->firstChild; $c !== null; $c = $c->nextSibling) {
            pb_walk($c, $out);
        }
        return;
    }
    // Attributes and remaining node kinds are not reachable here.
}

/** Canonical /root/child[n] path of a node, used for domxpath node-sets. */
function pb_node_path(DOMNode $n): string
{
    $parts = [];
    while ($n !== null && $n->nodeType !== XML_DOCUMENT_NODE) {
        $p = $n->parentNode;
        if ($p === null) {
            break;
        }
        $name = (string)($n->localName ?? $n->nodeName);
        $idx = 1;
        for ($s = $p->firstChild; $s !== null && $s !== $n; $s = $s->nextSibling) {
            if ($s->nodeType === $n->nodeType && (string)($s->localName ?? $s->nodeName) === $name) {
                $idx++;
            }
        }
        array_unshift($parts, $name . '[' . $idx . ']');
        $n = $p;
    }
    return '/' . implode('/', $parts);
}

function pb_xpath_result($res, string &$out): void
{
    if ($res instanceof DOMNodeList) {
        $out .= "N\x1f" . pb_s((string)$res->length) . "\x1e";
        foreach ($res as $node) {
            $out .= "I\x1f" . pb_s(pb_node_path($node))
                . pb_s((string)($node->localName ?? $node->nodeName))
                . pb_s((string)($node->namespaceURI ?? '')) . "\x1e";
        }
        return;
    }
    if (is_bool($res)) {
        $out .= "B\x1f" . pb_s($res ? '1' : '0') . "\x1e";
        return;
    }
    if (is_int($res)) {
        $out .= "D\x1f" . pb_s((string)$res) . "\x1e";
        return;
    }
    if (is_float($res)) {
        $out .= "D\x1f" . pb_s(pb_num($res)) . "\x1e";
        return;
    }
    if ($res === null) {
        $out .= "X\x1f" . pb_s('null') . "\x1e";
        return;
    }
    $out .= "S\x1f" . pb_s((string)$res) . "\x1e";
}

/* ------------------------------------------------------------------ */
/* ops                                                                 */
/* ------------------------------------------------------------------ */

function op_dom(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    $doc = new DOMDocument();
    if (!@$doc->load($file, LIBXML_NONET)) {
        return pb_fail('DOMDocument::load failed: ' . pb_err_text());
    }
    $buf = '';
    pb_walk($doc, $buf);
    $fp = hash('sha256', $buf);
    unset($doc);

    for ($i = 0; $i < $warmup; $i++) {
        $d = new DOMDocument();
        @$d->load($file, LIBXML_NONET);
        unset($d);
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $d = new DOMDocument();
        $t0 = hrtime(true);
        $ok = @$d->load($file, LIBXML_NONET);
        $ms = (hrtime(true) - $t0) / 1e6;
        unset($d);
        if (!$ok) {
            return pb_fail('DOMDocument::load failed on rep ' . $i . ': ' . pb_err_text());
        }
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp);
}

/** Ordered start/end element-name + text-length stream for XMLReader. */
function pb_reader_stream(string $file): ?string
{
    $r = new XMLReader();
    if (!@$r->open($file, null, LIBXML_NONET)) {
        @$r->close();
        return null;
    }
    $out = '';
    while (@$r->read()) {
        $t = $r->nodeType;
        if ($t === XMLReader::ELEMENT) {
            $out .= "E\x1f" . pb_s((string)($r->localName ?? '')) . "\x1e";
        } elseif ($t === XMLReader::END_ELEMENT) {
            $out .= "Z\x1f" . pb_s((string)($r->localName ?? '')) . "\x1e";
        } elseif ($t === XMLReader::TEXT || $t === XMLReader::CDATA
            || $t === XMLReader::WHITESPACE || $t === XMLReader::SIGNIFICANT_WHITESPACE) {
            $out .= "T\x1f" . pb_s((string)strlen((string)$r->value)) . "\x1e";
        } elseif ($t === XMLReader::COMMENT) {
            $out .= "C\x1f" . pb_s((string)strlen((string)$r->value)) . "\x1e";
        } elseif ($t === XMLReader::PI) {
            $out .= "P\x1f" . pb_s((string)$r->name) . "\x1e";
        }
    }
    @$r->close();
    return $out;
}

function op_xmlreader(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    $stream = pb_reader_stream($file);
    if ($stream === null) {
        return pb_fail('XMLReader::open failed: ' . pb_err_text());
    }
    $fp = hash('sha256', $stream);
    unset($stream);

    for ($i = 0; $i < $warmup; $i++) {
        pb_reader_stream($file);
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $t0 = hrtime(true);
        $s = pb_reader_stream($file);
        $ms = (hrtime(true) - $t0) / 1e6;
        if ($s === null) {
            return pb_fail('XMLReader::open failed on rep ' . $i . ': ' . pb_err_text());
        }
        unset($s);
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp);
}

function op_simplexml(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    $sx = @simplexml_load_file($file, 'SimpleXMLElement', LIBXML_NONET);
    if ($sx === false) {
        return pb_fail('simplexml_load_file failed: ' . pb_err_text());
    }
    $root = @dom_import_simplexml($sx);
    if (!($root instanceof DOMElement)) {
        return pb_fail('dom_import_simplexml failed: ' . pb_err_text());
    }
    $buf = '';
    pb_walk($root, $buf);
    $fp = hash('sha256', $buf);
    unset($root, $sx);

    for ($i = 0; $i < $warmup; $i++) {
        $s = @simplexml_load_file($file, 'SimpleXMLElement', LIBXML_NONET);
        unset($s);
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $t0 = hrtime(true);
        $s = @simplexml_load_file($file, 'SimpleXMLElement', LIBXML_NONET);
        $ms = (hrtime(true) - $t0) / 1e6;
        if ($s === false) {
            return pb_fail('simplexml_load_file failed on rep ' . $i . ': ' . pb_err_text());
        }
        unset($s);
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp);
}

function op_domxpath(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    $exprs = $families[$GLOBALS['pb_category']]['xpath'] ?? null;
    if (!is_array($exprs) || !$exprs) {
        return pb_fail('unknown category for xpath: ' . $GLOBALS['pb_category']);
    }
    $doc = new DOMDocument();
    if (!@$doc->load($file, LIBXML_NONET)) {
        return pb_fail('DOMDocument::load failed: ' . pb_err_text());
    }

    $xp = new DOMXPath($doc);
    $buf = '';
    foreach ($exprs as $ex) {
        $res = @$xp->evaluate((string)$ex);
        if ($res === false) {
            return pb_fail('DOMXPath::evaluate failed: ' . $ex);
        }
        pb_xpath_result($res, $buf);
    }
    $fp = hash('sha256', $buf);
    unset($xp, $buf);

    for ($i = 0; $i < $warmup; $i++) {
        $p = new DOMXPath($doc);
        foreach ($exprs as $ex) {
            @$p->evaluate((string)$ex);
        }
        unset($p);
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $p = new DOMXPath($doc);
        $t0 = hrtime(true);
        foreach ($exprs as $ex) {
            $res = @$p->evaluate((string)$ex);
            if ($res === false) {
                return pb_fail('DOMXPath::evaluate failed on rep ' . $i . ': ' . $ex);
            }
        }
        $ms = (hrtime(true) - $t0) / 1e6;
        unset($p);
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp, 'exprs=' . count($exprs));
}

function op_xsltprocessor(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    if (!class_exists('XSLTProcessor')) {
        return pb_fail('not_expressible: xsl extension not built');
    }
    $xsl_path = $bench . '/xslt/' . $GLOBALS['pb_category'] . '.xsl';
    if (!is_file($xsl_path)) {
        return pb_fail('missing stylesheet: ' . $GLOBALS['pb_category'] . '.xsl');
    }
    $doc = new DOMDocument();
    if (!@$doc->load($file, LIBXML_NONET)) {
        return pb_fail('DOMDocument::load failed: ' . pb_err_text());
    }
    $xsl = new DOMDocument();
    if (!@$xsl->load($xsl_path, LIBXML_NONET)) {
        return pb_fail('stylesheet load failed: ' . pb_err_text());
    }
    $proc = new XSLTProcessor();
    if (!@$proc->importStylesheet($xsl)) {
        return pb_fail('XSLTProcessor::importStylesheet failed');
    }
    $out = @$proc->transformToXml($doc);
    if (!is_string($out)) {
        return pb_fail('XSLTProcessor::transformToXml failed: ' . pb_err_text());
    }
    $fp = hash('sha256', $out);
    $out_bytes = strlen($out);
    unset($proc, $out);

    for ($i = 0; $i < $warmup; $i++) {
        $p = new XSLTProcessor();
        @$p->importStylesheet($xsl);
        @$p->transformToXml($doc);
        unset($p);
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $p = new XSLTProcessor();
        @$p->importStylesheet($xsl);
        $t0 = hrtime(true);
        $o = @$p->transformToXml($doc);
        $ms = (hrtime(true) - $t0) / 1e6;
        unset($p);
        if (!is_string($o)) {
            return pb_fail('XSLTProcessor::transformToXml failed on rep ' . $i . ': ' . pb_err_text());
        }
        unset($o);
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp, 'out_bytes=' . $out_bytes);
}

function op_serialize(string $file, int $reps, int $warmup, array $families, string $bench): array
{
    $doc = new DOMDocument();
    if (!@$doc->load($file, LIBXML_NONET)) {
        return pb_fail('DOMDocument::load failed: ' . pb_err_text());
    }
    $s = $doc->saveXML();
    if (!is_string($s)) {
        return pb_fail('DOMDocument::saveXML failed: ' . pb_err_text());
    }
    $fp = hash('sha256', $s);
    $bytes = strlen($s);
    unset($s);

    for ($i = 0; $i < $warmup; $i++) {
        @$doc->saveXML();
        libxml_clear_errors();
    }

    $best = INF;
    for ($i = 0; $i < $reps; $i++) {
        $t0 = hrtime(true);
        $s = @$doc->saveXML();
        $ms = (hrtime(true) - $t0) / 1e6;
        if (!is_string($s)) {
            return pb_fail('DOMDocument::saveXML failed on rep ' . $i . ': ' . pb_err_text());
        }
        unset($s);
        if ($ms < $best) {
            $best = $ms;
        }
        libxml_clear_errors();
    }
    return pb_ok($best, $fp, 'bytes=' . $bytes);
}

/* ------------------------------------------------------------------ */
/* main                                                                */
/* ------------------------------------------------------------------ */

$pb_results = [];
foreach ($pb_ops as $op) {
    try {
        switch ($op) {
            case 'dom':
                $pb_results[$op] = op_dom($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            case 'xmlreader':
                $pb_results[$op] = op_xmlreader($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            case 'simplexml':
                $pb_results[$op] = op_simplexml($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            case 'domxpath':
                $pb_results[$op] = op_domxpath($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            case 'xsltprocessor':
                $pb_results[$op] = op_xsltprocessor($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            case 'serialize':
                $pb_results[$op] = op_serialize($pb_file, $pb_reps, $pb_warmup, $pb_families, $pb_bench);
                break;
            default:
                $pb_results[$op] = pb_fail('unknown op: ' . $op);
        }
    } catch (Throwable $e) {
        $pb_results[$op] = pb_fail('exception: ' . $e->getMessage());
    }
    libxml_clear_errors();
}

$pb_obj = [
    'consumer' => 'php',
    'id'       => $pb_id,
    'category' => $pb_category,
    'file'     => $pb_file,
    'ops'      => $pb_results,
];

fwrite(STDOUT, json_encode($pb_obj, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE) . "\n");
