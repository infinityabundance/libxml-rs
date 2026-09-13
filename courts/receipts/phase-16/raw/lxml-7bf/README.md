# lxml court 7bf — zero failing ids, and the HTML teardown corruption is gone

This slice closes the last seven candidate-driven lxml 6.1.2 failures **and**
removes the intermittent glibc abort that made "zero failures" unverifiable.

* Full suite: `python3 test.py -u -v -v` -> **Ran 2007 tests, OK**; the raw unique
  failing id set is **empty** (`failing-ids.txt`, `suite-verbose.log`).
* `cargo test --lib`: 1333 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* 26+ consecutive `test_(etree|htmlparser)` runs and 8+ full-suite runs clean
  (previously the combined run aborted roughly 2 of every 3 times).

## 1. HTTP/HTTPS input loader (4 ids)

`test_http_client`, `test_http_client_gzip`, `test_network_dtd`,
`test_parser_input_mix`.

The input layer had no HTTP transport, so `http://` URLs failed to resolve. Added
`src/xml/io/http.rs`: a minimal HTTP/1.0 `GET` client handling redirects,
`Content-Encoding: gzip`/`deflate` and chunked transfer-decoding, wired into
`input_buffer_create_file` (`src/xml/io/mod.rs`) and `input_from_file`
(`src/xml/parser/helpers.rs`). `flate2` (pure-Rust `miniz_oxide` backend) avoids a
system-zlib link requirement.

## 2. isoschematron (3 ids)

`test_schematron_phases`, `test_schematron_phases_kwarg`,
`test_schematron_relaxng_embedded`.

* `document()` now treats an **empty node-set** argument as an empty document
  (upstream parity), extracted into `xslt_document_load`
  (`src/xslt/transform/mod.rs`); `xsltDocumentFunction`'s empty-node-set guard was
  reordered to match (`src/abi/exports_xslt_functions.rs`).
* `current()` now returns the **transform context node** `(*tctxt).node` instead of
  `ctx.context_node`.

## 3. HTML teardown corruption (no failing id, but the blocker)

### Symptom

`test_(etree|htmlparser)` aborted ~2/3 of the time with `free(): double free
detected in tcache 2` or `malloc_consolidate(): unaligned fastbin chunk detected`.
The gdb backtrace always ended in lxml's `_fixHtmlDictNames` freeing a node name.
A single `test.py` invocation must not be split across modules, and isolated
reproducers were clean — the corruption is heap-layout dependent.

### Diagnosis

An env-gated registry of live `xmlDict` strings (`entry.data`) was added so that
any free of a *live* dictionary-owned pointer would abort at its **first**
occurrence with the owning dict and a Rust backtrace. It fired at
`test_html_fromstring_target_exceptions`:

```
free_node: live dict string 0x...5e80 (name="html") owned by dict 0x...6c70;
           node=0x... doc=0x... doc.dict=0x0
```

`free_node` (`src/xml/tree/mod.rs`) uses `doc->dict` for its `DICT_FREE` guard
(exactly like upstream). The document had `dict == NULL`, yet its node names were
interned into a live dict — so teardown freed live dictionary strings, and a later
lookup returned a dangling pointer that was freed again.

### Root cause

The hosted whole-document HTML read entry points created the document internally
without ever firing the SAX `setDocumentLocator` + `startDocument` hooks. lxml
installs `_initSaxDocument` as `sax.startDocument`; that hook is what adopts
`ctxt->dict` as `doc->dict` (and references it). Upstream `htmlParseDocument`
fires these hooks before parsing the tree. Without them, `_fixHtmlDictNames(ctxt.dict,
doc)` then interned every name into `ctxt.dict` while `doc.dict` stayed `NULL`.
The target-parser path (`_TargetParserContext._handleParseResultDoc`) calls
`xmlFreeDoc(result)` directly, without lxml's `initDocDict` repair, so it tripped
first.

### Fix

`parse_memory_enc_hosted` (`src/xml/html/mod.rs`) now dispatches
`setDocumentLocator` + `startDocument` before parsing and parses into the document
that hook created, mirroring `htmlParseDocument`. All hosted entry points route
through it (`src/abi/exports_html.rs`): `htmlCtxtReadMemory`, `htmlCtxtReadDoc`,
`htmlCtxtReadFile`, `htmlCtxtReadFd`, `htmlCtxtReadIO` and
`htmlCtxtParseDocument`. Non-hosted convenience readers (`htmlReadMemory`, …) have
no consumer SAX and are unchanged.

### Verification

With the invariant check armed, 26 consecutive `test_(etree|htmlparser)` runs, 3
full suites, and targeted probes for `fromstring` (unicode/bytes), file-object and
file-path target parses were all clean; previously `etree.parse(path,
HTMLParser(target=…))` also reproduced the bad free, and now does not.

The diagnostic registry was removed before commit; the fix stands on its own
(structural parity, not a guard).

## Evidence files

* `run.txt` — machine-readable slice record.
* `failing-ids.txt` — empty id set.
* `suite-summary.txt` — fixed/stability/regression summary.
* `suite-verbose.log` — `python3 test.py -u -v -v` output (Ran 2007 tests, OK).
