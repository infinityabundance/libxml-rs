# §16.12 consumer driver interface (fixed)

Every consumer driver is invoked **inside the perf container** after the
provider has been selected by sourcing the provider environment. It emits one
single-line JSON object on stdout and nothing else.

## Invocation

```
<driver> --id <id> --category <family> --file <abs-path> --reps <N> [--warmup <W>] [--ops <a,b,c>]
```

* `<driver>` — `cli_xmllint.sh`, `cli_xsltproc.sh`, `lxml_driver.py`,
  `nokogiri_driver.rb`, `php_driver.php`.
* `--file` — absolute path to the (uncompressed) corpus member.
* `--category` — corpus family, key into `families.json` (selects the family's
  XPath expressions and XSLT stylesheet).
* `--reps` — timed repetitions (the driver reports the **best** time).
* `--ops` — restrict to these comma-separated ops (default: the consumer's full
  op set).

## Provider selection (done by the caller, not the driver)

```sh
source /court/consumers/lib.sh oracle      # upstream at /usr/local
source /court/consumers/lib.sh candidate   # libxml-rs at /candidate
```

`LD_LIBRARY_PATH`/`PATH`/`PKG_CONFIG_PATH` then resolve to the provider's
libxml2/libxslt/libexslt and CLIs. The driver must not change them.

## Output

```json
{"consumer":"lxml","id":"svg-001","category":"SVG","file":"/corpus/svg-001.svg",
 "ops":{"dom_parse":{"ok":true,"ms":0.41,"fingerprint":"sha256:…","detail":""}}}
```

* `ms` — in-process **engine** time in milliseconds for one operation, best of
  `--reps` after `--warmup` warmups. For process-CLI consumers (xmllint,
  xsltproc) `ms` may be `null`; the orchestrator measures process wall time.
* `fingerprint` — `sha256:<hex>` of a canonical UTF-8 string that captures the
  operation's observable result (see below). It must be comparable across
  providers running against the *same* file.
* `ok` — whether the operation ran. On failure: `{"ok":false,"error":"…"}`.
* `detail` — optional short human note.

## Fingerprints (§16.14)

| Op class | Fingerprint input |
|---|---|
| parse (DOM) | canonical serialization of every node: kind, element/attr local names, namespace URI, attribute values, text, document order |
| streaming | ordered sequence of start/end element names + text lengths |
| xpath | result type + cardinality + each result item (string value or node path) |
| serialize | whatever is serialized, hashed |
| validation | verdict (`valid`/`invalid`) + normalized error kind |
| xslt | the transform output bytes |

Rules: no timestamps, no pointers, no absolute paths, no provider/version
strings — the fingerprint must be identical for oracle and candidate when the
semantic result matches. If an operation legitimately cannot be expressed by a
CLI consumer, emit `{"ok":false,"error":"not_expressible: …"}` (recorded, never a
timing).

## Resource mount layout inside the perf container

| Path | Contents |
|---|---|
| `/corpus/<file>` | the 100 corpus members (read-only) |
| `/bench` | these drivers + `families.json` + `xslt/` (read-only) |
| `/candidate` | candidate release build (lib + bin) |
| `/usr/local` | upstream oracle (lib + bin) |
| `/court/consumers/lib.sh` | provider selector |
| `/out` | writable scratch / evidence |
