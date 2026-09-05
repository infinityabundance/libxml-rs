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

## debian — not yet gated

Multi-distro Dockerfiles exist (`courts/suites/phase14/docker/Dockerfile.{debian,ubuntu,opensuse,arch,almalinux}`)
and there is a stale `courts/receipts/phase-14/debian-lxml-out` from the Sep-1
bootstrap attempt (segfaulted). No current reverse-dependency census/runner has
been produced.

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
