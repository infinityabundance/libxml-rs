# Phase 15.1 — lxml court baseline (first full-suite gate)

**Date:** 2026-09-05
**Consumer:** lxml 6.1.2 (pinned `lxml-6.1.2`), built against the candidate
`target/debug` DSOs.
**Oracle:** upstream libxml2 2.15.3 + libxslt 1.1.45 at `/usr/local`
(`lxml-oracle` container). **Oracle passes every module below (rc=0)** — every
candidate failure listed is candidate-driven, not oracle-parity.

## Gate definition

`python3 test.py -u` (unit tests) run per-module in `lxml-cand` with
`PYTHONPATH=/src/lxml/src LD_LIBRARY_PATH=/candidate/lib` against the freshly
built candidate DSO. The canonical in-container runner is
`courts/suites/phase14/consumers/lxml-run.sh <oracle|candidate>`.

## Result

- **Passing modules** (rc=0): `test_annotations`, `test_builder`,
  `test_classlookup`, `test_css`, `test_elementtree` (612 — fixed in 0b66ebc1),
  `test_errors`, `test_external_document`, `test_nsclasses`,
  `test_pyclasslookup`, `test_sax`, `test_unicode`.
- **`test_etree`**: passes its 295 ordinary tests; the suite only *appears* to
  hang on `test_very_large_sourceline_iterparse`, a 2.2 GB stress test that is
  inherently slow (not a candidate-specific hang).
- **`test_http_io`** (4 errors): `test_http_client`, `test_http_client_gzip`,
  `test_network_dtd`, `test_parser_input_mix` — network/environment dependent
  in the sandbox, not yet classified as parity.

## Failure counts (candidate-driven)

| module | failures | errors | cluster |
|---|---:|---:|---|
| `test_xslt` | 33 | 18 | XSLT/EXSLT engine |
| `test_isoschematron` | 2 | 21 | ISO Schematron (XSLT-based) |
| `test_htmlparser` | 19 | 3 | HTML parser |
| `test_xpathevaluator` | 10 | 2 | XPath / EXSLT functions |
| `test_dtd` | 7 | 5 | DTD validation |
| `test_xmlschema` | 11 | 0 | XSD validation |
| `test_incremental_xmlfile` | 6 | 4 | incremental writer |
| `test_relaxng` | 7 | 0 | RELAX NG validation |
| `test_objectify` | 2 | 4 | objectify object path |
| `test_elementpath` | 4 | 0 | XPath elementpath find() |
| `test_threading` | 2 | 0 | thread-local XSLT error log |
| `test_io` | 1 | 1 | UTF-16 BOM iterparse / filename percent |
| `test_schematron` | 1 | 0 | Schematron |
| `test_doctestcompare` | 1 | 0 | HTML case doctest |
| **total** | **106** | **58** | |

## Root-cause clusters (triage)

1. **XSLT/EXSLT engine** (`test_xslt` 51, `test_isoschematron` 23,
   `test_threading` 2, EXSLT slices of `test_xpathevaluator`): extension
   elements/functions (`test_extension_element*`, `test_extensions*`),
   EXSLT modules (`test_exslt_math`, `test_exslt_str`,
   `xpath_exslt_functions_date/strings`), parameters
   (`test_xslt_parameter*`, `test_xslt_parameters`,
   `test_xslt_string_parameters`, `test_xslt_multiple_parameters`),
   `document()`, `message`/`terminate`, apply/parsing error-log in threads.
   Many transformations return `None` (empty result tree), pointing at
   extension/EXSLT registration and parameter binding gaps. **Largest and
   deepest cluster.**

2. **Validation engines** (`test_dtd` 12, `test_xmlschema` 11,
   `test_relaxng` 7, `test_schematron` 1): `assertValid`/`validate` not
   rejecting invalid documents, duplicate-ID detection, XSD default/fixed
   attributes, error-log wording, stringio/shortcut paths.

3. **HTML parser** (`test_htmlparser` 22): feed/iterparse/pull parser,
   boolean-attribute round-trip, target-parser doctype/exception paths.

4. **XPath** (`test_xpathevaluator` 12, `test_elementpath` 4): context node,
   class errors, compile errors, `find()`, EXSLT date/string functions.

5. **Incremental writer** (`test_incremental_xmlfile` 10): encoding, PI
   handling, xml-mode-inside-html.

6. **Objectify** (`test_objectify` 6): object path add/set attribute.

7. **Misc small** (`test_io` 2, `test_doctestcompare` 1).

## Priority

1. XSLT/EXSLT cluster (highest count; likely a few shared root causes that
   unblock many tests at once).
2. Validation cluster (default/fixed attributes, duplicate ID, error log).
3. HTML parser cluster.
4. XPath cluster.
5. Remaining smaller clusters, then re-run the canonical full suite and seal.
