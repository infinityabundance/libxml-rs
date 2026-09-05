# Phase 15 — four-consumer drop-in status (lxml / php / nokogiri / debian)

**Date:** 2026-09-05. Goal: 0 failures on all four consumers = full drop-in
replacement. Oracle = upstream libxml2 2.15.3 + libxslt 1.1.45 at `/usr/local`.

## php — 0 failures (sealed, Phase 14)

Six-extension gate (dom/simplexml/xml/xmlreader/xmlwriter/xsl): **0 failed**
under NTS and ZTS. Not re-run this phase; must be re-gated after any shared
engine change.

## lxml — 164 candidate-driven failures (see LXML-COURT-BASELINE.md)

`test.py -u` per-module: oracle passes every module; candidate has 106 failures
+ 58 errors across 14 modules. Largest cluster is XSLT/EXSLT engine
(`test_xslt` 51 + `test_isoschematron` 23 + …).

## nokogiri — oracle 0 failures, candidate CRASHES + failures

- **Oracle** (`nokogiri-run.sh oracle` + `bundle exec rake test`): 2831 runs,
  10268 assertions, **0 failures, 0 errors, 39 skips**.
- **Candidate** (`bundle exec rake test`, `LD_LIBRARY_PATH=/candidate/lib`):
  multiple **SIGSEGV** crashes and many failures.
- Two distinct crash sites observed:
  1. XPath extension-function dispatch:
     `_noko_xml_xpath_context__xpath2ruby` (reads `c_context->doc`) ←
     `Nokogiri_marshal_xpath_funcall_and_return_values` ←
     `invoke_c_extension_function` (src/xml/xpath/eval.rs:553).
  2. SAX parser context creation: `native_memory` (nokogiri
     `xml/sax/parser_context.rb:78`) — empty-processing-instruction test.
- CSS integration: `Nokogiri::HTML5::Document` selector tests fail broadly
  (CSS→XPath on HTML5 documents).
- Isolated `test/xml/test_xpath_context.rb` (no crash): 2 failures —
  `SyntaxError expected but nothing was raised` for register/deregister
  namespaces and variables.
- Minimal C probes (`xmlXPathRegisterFunc` and `xmlXPathRegisterFuncLookup`
  with string/number/node-set args) **pass** on the candidate — the XPath
  crash is specific to nokogiri's Ruby `VALUE` marshaling path, not the basic
  C extension-function protocol. **Root cause not yet isolated.**

### nokogiri infrastructure note

nokogiri v1.19.4 requires Ruby >= 3.2; the base image ships Ruby 3.1. Built
Ruby 3.2.3 into a new image `libxml-rs/phase14-debian-ruby32:1` and the
containers `nokogiri-oracle` / `nokogiri-cand`. The committed
`nokogiri-run.sh` uses `rake test` (not `bundle exec rake test`); with the
pinned Gemfile (minitest 6.0.1 + minitest-mock 5.27.0) the non-bundled
`rake test` cannot load `minitest/mock`, so the runner needs `bundle exec`.

## debian — reverse-dependency drop-in: 175 missing symbols + SONAME mismatch

Fresh `debian:bookworm` container (`debian-court`), system libxml2 2.9.14 +
libxslt 1.1.35. Reverse-deps installed: `libxml2-utils` (xmllint),
`xsltproc`, `python3-lxml`, `ruby-nokogiri`, `php-cli php-xml`.

Drop-in dir `/dropin` = candidate versioned `libxml2.so.2` (R-000179 rebuild) +
`libxslt.so.1` + `libexslt.so.0` facades.

### Result

- **175 symbols exported by Debian `libxml2.so.2` are missing from the
  candidate** (system 1786 vs candidate 1748). They are genuinely absent from
  the unversioned core too (0 overlap), i.e. a real implementation gap, not a
  version-script-only issue.
  - `xmlBuf*` buffer API (~45): `xmlBufAdd/Create/Free/Length/Grow/…`
  - legacy SAX1 handler globals: `startElement`, `characters`, `attribute`,
    `endElement`, `comment`, `getEntity`, …
  - parser internals: `inputPush/inputPop/namePush/namePop/nodePush/nodePop/
    valuePush/valuePop`
  - global vars + `__` aliases: `xmlGenericError`, `xmlLastError`,
    `xmlIndentTreeOutput`, `__xmlParserVersion`, `__xmlRegisterCallbacks`, …
  - encoding: `UTF8ToHtml/UTF8Toisolat1/isolat1ToUTF8`,
    `xmlEncodeAttributeEntities`, `htmlDecodeEntities`
  - xz: `__libxml2_xz*`; misc: `xmlAutomataSetFlags`, `xmlCatalogDumpDoc`,
    `xmlXPtrAdvanceNode`, `libxml_domnode_*_sort`, …
- **Drop-in load failures** (even before any runtime test):
  - `xmllint` → `undefined symbol: inputPush` (LIBXML2_2.4.30)
  - `php -r 'new DOMDocument()'` → `undefined symbol: __xmlParserVersion`
  - `xsltproc` / `python3-lxml` / `ruby-nokogiri` → `libxml2.so.16: cannot
    open` (the candidate libxslt/libexslt facades NEED the unversioned
    `libxml2.so.16`, but the drop-in only supplies `libxml2.so.2`).

So the Debian reverse-dependency court's first-order failure count is **175
missing symbols + 1 SONAME-mismatch**, and every installed reverse-dep fails to
load. Fixing this is a prerequisite before any runtime reverse-dep test can run.

### ltrace residual logging (which missing symbols consumers actually CALL)

`ltrace -e '@libxml2.so.2*'` over the five reverse-deps (xmllint, xsltproc,
python3-lxml, ruby-nokogiri, php) yields **70 unique runtime-called libxml2
symbols**. Of those, **~40 are missing from the candidate** — the critical,
actually-exercised subset of the 175, dominated by:

- `xmlBuf*` buffer API (xmlBufCreate/CreateSize/Add/AddLen/Avail/Free/
  GetAllocationScheme/Grow/ResetInput/SetAllocationScheme, xmlAllocOutputBufferInternal)
- `__xml*` globals/aliases (`__xmlDefaultSAXHandler`, `__htmlDefaultSAXHandler`,
  `__xmlDefaultBufferSize`, `__xmlBufferAllocScheme`, `__xmlLastError`,
  `__xmlGenericError`, `__xmlIndentTreeOutput`, `__xmlSaveNoEmptyTags`,
  `__xmlSubstituteEntitiesDefaultValue`, `__xmlLineNumbersDefaultValue`,
  `__xmlKeepBlanksDefaultValue`, `__xmlLoadExtDtdDefaultValue`,
  `__xmlDoValidityCheckingDefaultValue`, `__xmlPedanticParserDefaultValue`,
  `__xmlParserDebugEntities`, `__xmlParserInputBufferCreateFilenameValue`,
  `__xmlOutputBufferCreateFilenameValue`, `__xmlStructuredError`,
  `__xmlStructuredErrorContext`, `__xmlRandom`, `__xmlInitializeDict`,
  `__xmlGlobalInitMutex{Lock,Unlock,Destroy}`)
- xz (`__libxml2_xz{open,read,close,compressed}`)
- `initGenericErrorDefaultFunc`

### Candidate STUB functions (exported but not implemented)

`// Phase 1: STUB` marks in `src/abi/exports_xml2.rs` (7 found): `htmlParseFile`,
`htmlParseMemory`, `htmlParseDoc`, `htmlInitParser`, `htmlCleanupParser`,
`xmlGetBinaryPath`, `xmlGetHomeOfBinary`. The three `htmlParse*` stubs return
NULL — a real gap for any consumer using the legacy HTML parse entry points
(though lxml/nokogiri use `htmlRead*`/`htmlCtxtRead*`, not these).

## Priority (biggest movers first)

1. **Isolate and fix the nokogiri XPath extension-function SIGSEGV** — it
   blocks nokogiri entirely and very likely underlies a large slice of lxml's
   XSLT/EXSLT/XPath extension failures.
2. **XSLT/EXSLT engine** — largest lxml cluster, likely shared with nokogiri's
   XSLT/extension surface.
3. **Validation cluster** (DTD/XSD/RelaxNG/Schematron) — shared across lxml,
   nokogiri, php.
4. HTML parser, XPath, incremental writer, objectify (lxml).
5. Stand up the Debian reverse-dependency court, then cross-triage.
6. Re-gate php + nokogiri after every shared-engine change to guarantee no
   regression.
