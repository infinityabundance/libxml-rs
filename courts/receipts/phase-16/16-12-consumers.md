# Phase 16.12 — five consumer performance suites: receipt

Commit: (this commit)
Date: 2026-09-14
Phase: 16.12 (§16.12.1–§16.12.5). Corpus: 100 files / 8.41 GiB. Eligibility:
`courts/receipts/phase-16/corpus-eligibility.json` revision 2.

Raw evidence:

| Artifact | Path |
|---|---|
| Combined matrix + aggregation (2 611 cells) | `courts/receipts/phase-16/16-12-consumer-matrix.json` |
| Discovered defects (machine-readable) | `courts/receipts/phase-16/16-12-findings.json` |
| Per-cell raw results (gitignored, 5 222 files) | `courts/receipts/phase-16/raw/16-12/` |
| Fixed drivers | `tools/bench/consumers/{cli_driver.py,lxml_driver.py,nokogiri_driver.rb,php_driver.php,xsltperf.c}` |
| Orchestrator + equivalence + aggregation | `tools/bench/consumer_matrix.py` |
| Runtime build/stage | `courts/suites/phase16/consumers-setup.sh` |
| Family queries/transforms | `tools/bench/consumers/families.json`, `tools/bench/consumers/xslt/*.xsl` |

**Headline.** All five consumers (xmllint, xsltproc, python3-lxml, ruby-nokogiri,
php) were built/staged against **both** providers (upstream libxml2 2.15.3 /
libxslt 1.1.45 at `/usr/local`; the libxml-rs candidate at `/candidate`) and run
over the **whole 100-file corpus** for every §16.11-eligible
(consumer, operation) pair: **2 611 cells**. Result equivalence (§16.14) was
enforced before any timing was accepted:

| Outcome | Cells |
|---|---|
| equivalent (timing valid) | **2 123** |
| INVALID_RESULT (both parsed, different result) | 142 |
| asymmetric error (one provider failed/timed out) | 242 |
| not expressible offline (no DTD/XSD resource, provider-side error) | 104 |

The run also **found eight real candidate defects** (below). This is the corpus
doing its job: the apparatus caught divergences that the synthetic and
phase-14 corpora did not.

Reproduce:

```sh
sh courts/suites/phase16/consumers-setup.sh
python3 tools/bench/consumer_matrix.py --all
python3 tools/bench/gen_findings.py
```

---

## 1. Method (fixed before the run)

* **Drivers** are fixed and provider-agnostic; provider switching is
  `source /court/consumers/lib.sh oracle|candidate` (PATH/LD_LIBRARY_PATH/
  PKG_CONFIG_PATH). Verified per provider by `ldd`/`/proc/self/maps`.
* **Timing.** Every CLI sample includes process startup (a valid CLI consumer
  metric). In-process consumers (lxml, nokogiri, php) report the **best of
  `--reps` monotonic engine timings** after warmups; repetitions by size:
  ≤256 KiB → 5, ≤4 MiB → 3, larger → 1. Wall time and max RSS are captured per
  run.
* **Equivalence fingerprints** are canonical and provider-independent: xmllint
  uses **c14n** (canonical XML); lxml/nokogiri/php use a canonical DOM walk
  (node kinds, element/attr names, namespace URIs, values, text, order) or the
  event stream (sax/reader/iterparse/xmlreader); XPath uses result type +
  cardinality + items; XSLT/serialize use output bytes. A per-op **instability
  check** flags outputs that change across reps instead of recording a fake win.
* **Per-op wall budget**, sized by input (`max(15, min(90, bytes/20 MB))` s),
  is fixed before the run; exceeding it is recorded as a timeout, never skipped.
* **Aggregation** obeys the frozen §16.11 `aggregation_policy`: the full
  per-cell distribution plus **macro (per-file)** and **micro (byte/time-
  weighted)** summaries and **category-** and **size-bucket-stratified**
  breakdowns. A single unstratified overall speedup is not the headline.

`candidate_speedup = oracle_ms / candidate_ms` (>1 means the candidate is
faster).

## 2. Aggregate results (all values in the matrix)

| consumer / operation | n | macro (per-file) | micro (byte-weighted) |
|---|---|---|---|
| xmllint / parse | 79 | **1.059** | 0.566 |
| xmllint / serialize | 79 | **1.032** | 0.337 |
| xmllint / stream | 79 | **1.012** | 0.261 |
| xmllint / xpath | 90 | **1.543** | 4.046 |
| xmllint / xsd_validate | 10 | 0.764 | 0.764 |
| xsltproc / transform | 79 | 0.561 | 0.024 |
| xsltproc / stylesheet_compile | 91 | 0.459 | 0.472 |
| xsltproc / precompiled_apply | 79 | 0.155 | 0.009 |
| python3-lxml / dom_parse | 93 | 0.687 | 0.648 |
| python3-lxml / iterparse | 95 | 0.515 | 0.437 |
| python3-lxml / tostring | 11 | 0.817 | 0.665 |
| python3-lxml / xpath_adhoc | 82 | 0.267 | 0.008 |
| python3-lxml / xpath_compiled | 82 | 0.271 | 0.008 |
| python3-lxml / xpath_compile | 98 | 0.598 | 0.687 |
| python3-lxml / xslt | 79 | 0.161 | 0.009 |
| python3-lxml / xsd_validate | 10 | 0.511 | 0.509 |
| ruby-nokogiri / dom_parse | 79 | 0.472 | 0.556 |
| ruby-nokogiri / sax_parse | 91 | 0.722 | 0.739 |
| ruby-nokogiri / reader | 91 | 0.650 | 0.320 |
| ruby-nokogiri / xpath | 79 | 0.363 | 0.024 |
| ruby-nokogiri / serialize | 80 | 0.902 | 0.594 |
| ruby-nokogiri / xslt | 78 | 0.163 | 0.012 |
| php / dom | 80 | 0.440 | 0.475 |
| php / xmlreader | 92 | 0.481 | 0.272 |
| php / simplexml | 79 | 0.449 | 0.362 |
| php / domxpath | 79 | 0.261 | 0.017 |
| php / xsltprocessor | 79 | 0.145 | 0.009 |
| php / serialize | 80 | 0.859 | 0.725 |

Stratified views (the contract's requirement) are in the matrix under
`aggregation[*].{category_stratified,size_bucket_stratified}`. Worked example
(the macro/micro divergence the contract exists to expose):

```
xmllint/parse       macro 1.059  micro 0.566
  by bucket: t0 1.68 | t1 1.01 | t2 0.46 | t3 0.58 | t4 0.57
  by family: MAVEN 1.58 SVG 1.52 RSS 1.42 DOCBOOK 1.15 AOSP 1.02
             GPX-KML 1.07 TEI 0.88 JATS 0.84 SEC-XBRL 0.65 MUSICXML 0.48
```

The candidate is **at parity or faster on the xmllint CLI paths** (startup-
inclusive parse/serialize/stream macro ≈ 1.0–1.06) and on tiny documents, but
**loses broadly on the library paths** (DOM ~0.44–0.69, XPath, XSLT 0.15–0.16,
namespace-heavy). XSLT and XPath micro values collapse toward 0 because the
candidate times out or slows drastically on larger inputs (findings F4/F5).

## 3. Result equivalence and the invalid population

* **142 INVALID_RESULT** across 22 files; **17 of the 22 contain `&amp;` inside
  an attribute**, and F1 explains 114 of the 142.
* **242 asymmetric** cells: 84 candidate `timeout after 15s`, 80
  `serialization_instability`, 15 candidate `DTDParseError`, plus XSLT/library
  timeouts. 2 oracle timeouts (the 2.3/2.7 GB files under a fixed budget).
* **104 not expressible offline**: no DTD resource (14), no XSD resolver (10),
  and provider-side parse/read errors (both providers fail; no timing).
* Serialization **byte** comparison (xmllint `--format`): 21/95 byte-identical,
  **74 differ** (F3).

## 4. Discovered candidate defects (in `16-12-findings.json`)

| ID | Severity | Summary | Cells | Minimal repro |
|---|---|---|---|---|
| **F1** | correctness | `&amp;` in an **attribute value** decodes to the literal `&#38;` instead of `&` (text nodes are correct) | 114 | `xmllint --xpath 'string(//a/@b)'` on `<a b="x&amp;y">` → oracle `x&y`, candidate `x&#38;y` |
| **F2** | correctness | Serializing the **same tree twice** returns an empty string the second time | 80 | `etree.tostring(t)` twice: oracle 30837,30837; candidate 30837,**0** |
| **F3** | serialization | `xmllint --format` indentation/blank-line divergence | 74 files | `xmllint --format /corpus/svg-001.svg` |
| **F4** | performance | XPath **super-linear**: >60 s or non-terminating on a 12 MB file (oracle <1 s) | 37 | `timeout 60 xmllint --xpath 'count(//*[local-name()="node"])' /corpus/gpxkml-007.gpx` |
| **F5** | performance | XSLT application times out where the oracle completes quickly | 41 | `xsltproc <xsl> <file>` on several families |
| **F6** | correctness | Canonical form differs for a KML doc with **CDATA** descriptions | 22 | `xmllint --c14n gpxkml-003.kml` → 37286 vs 38051 B |
| **F7** | correctness | Canonical form differs for an SVG doc with an **internal DTD subset** (defaulted attributes/entities) | 3 | `xmllint --c14n svg-006.svg` → 108562 vs 101061 B |
| **F8** | correctness | lxml **DTD parse** fails under the candidate where the oracle parses the same DTD | 15 | `etree.DTD('/bench/schema/partwise.dtd')` |

F1 is the single highest-leverage fix: it is a one-line entity-decoding defect in
attribute values and it accounts for the majority of invalid results. F2 (repeat
serialization) is a use-after-free-style state bug. F4/F5 are the performance
frontier the rest of Phase 16 exists to close.

## 5. Limitations

* **Two largest files (2.3/2.7 GB)** exceeded the fixed budget on two oracle
  cells; recorded as timeouts, not skipped.
* **Offline validation resources**: only Maven XSD, MusicXML DTD and SVG DTD
  are materialized (`tools/bench/consumers/fetch_schemas.py`); JATS/DocBook/
  PubMed DTDs are recorded but not resolvable offline, so those validation cells
  are `not_expressible` (14+10), not measured.
* `python3-lxml/tostring`, `xslt` and the XSLT-bearing cells have small `n`
  because F2/F5 remove the candidate side; those cells are INVALID/asymmetric,
  not excluded by the corpus.
* XPath shows **high variance** (e.g. `xmllint/xpath` micro 4.05 driven by one
  OSM cell) alongside the timeouts; the XPath path needs the F4 fix before its
  numbers are stable. Reported as-is; no result was discarded.

## 6. Next phase

*16.13* materializes the remaining real queries/transforms (upstream TEI/DocBook
stylesheets; JATS/DocBook/PubMed DTDs) and *16.14* formalises the equivalence
court. The immediate engineering priority raised by this phase is **F1**
(correctness) and **F4/F5** (performance); both invalidate large parts of the
matrix until fixed.
