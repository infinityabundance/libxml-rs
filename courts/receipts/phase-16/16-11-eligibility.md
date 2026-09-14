# Phase 16.11 — freeze consumer eligibility before timing: receipt

Commit: (this commit)
Date: 2026-09-13
Phase: 16.11. Corpus manifest SHA-256 at freeze:
`5bb81e3e9334bb8e58ea12eaeb92ac2c58369aa8dfab722cbe221699a864a75b`.
Frozen at `2026-09-14T00:37:24Z`, **revision 2** (revision 1 was raised via an
explicit pre-16.12 amendment; see §4).

Raw evidence:

| Artifact | Path |
|---|---|
| Eligibility matrix of record (frozen) | `courts/receipts/phase-16/corpus-eligibility.json` |
| Policy engine + freeze/amendment gate | `tools/bench/corpus_eligibility.py` |
| Input corpus | `tools/bench/corpus/manifest.json`, `courts/receipts/phase-16/corpus-report.json` |

**Headline result.** All 100 corpus members received a `(consumer, operation)`
eligibility matrix computed **before any candidate timing existed**, from the
manifest and the predeclared resource/policy tables alone. The freeze is
verifiable and fail-closed: `corpus_eligibility.py --check` recomputes the
matrix and fails on any drift (wired into CI). After measurement begins, a
change requires `--amend "<rationale>"`, which bumps the revision and appends an
amendment receipt.

Reproduce:

```sh
python3 tools/bench/corpus_eligibility.py --summary     # compute/refresh
python3 tools/bench/corpus_eligibility.py --check       # verify frozen seal
python3 tools/bench/corpus_eligibility.py --amend "..."  # explicit amendment
```

---

## 1. The anti-cherry-picking guarantee (§16.11)

Eligibility is a pure function of five inputs. It is computed from
`manifest.json` + `corpus-report.json` + the embedded policy/resource tables
and **never reads a timing of any kind** — the file records
`"performance_data_used": false`. A file therefore cannot be excluded because
the candidate loses on it; exclusion follows only from support, semantic
appropriateness, document dependencies, format, or resource availability.

| Spec basis | How it is applied |
|---|---|
| **Consumer API support** | Fixed per-consumer operation catalog (§16.12); an operation exists only where the pinned consumer actually exposes it |
| **Semantic appropriateness** | No validation without a target; DOM-bound XSLT and SimpleXML are capped on huge documents; DOM/streaming parsing stays open at every size |
| **Document dependencies** | Manifest `doctype` (DTD target) and `schema_deps` (XSD target) |
| **Data format** | All members are XML (RSS 1.0 RDF/XML included); no format-driven exclusion |
| **Available stylesheet/schema/query** | Per-family `xpath` and `xslt` declarations; the one available instance-XSD target (Maven POMs) |

## 2. Operation catalog per consumer (§16.12)

| Consumer | Operations |
|---|---|
| `xmllint` | parse, stream, xpath, dtd_validate, xsd_validate, relaxng_validate, serialize |
| `xsltproc` | transform, stylesheet_compile, precompiled_apply |
| `python3-lxml` | dom_parse, iterparse, xpath_adhoc, xpath_compiled, xpath_compile, tostring, xslt, dtd_validate, xsd_validate |
| `ruby-nokogiri` | dom_parse, sax_parse, reader, xpath, serialize, xsd_validate, xslt |
| `php` | dom, xmlreader, simplexml, domxpath, xsltprocessor, serialize |

`relaxng_validate` is in the catalog but eligible nowhere: no corpus member has
a real RelaxNG resource, and the spec forbids using a validation mode without a
real validation target.

## 3. The frozen matrix

Eligible-file counts per operation:

| Consumer | parse-class | xpath-class | dtd_validate | xsd_validate | xslt class | serialize |
|---|---|---|---|---|---|---|
| `xmllint` | parse 100, stream 100 | 100 | 22 | 10 | — | 100 |
| `xsltproc` | — | — | — | — | 91 (transform/compile/apply) | — |
| `python3-lxml` | dom_parse 100, iterparse 100 | 100 (adhoc/compiled/compile) | 22 | 10 | 91 | tostring 100 |
| `ruby-nokogiri` | dom 100, sax 100, reader 100 | 100 | — | 10 | 91 | 100 |
| `php` | dom 100, xmlreader 100 | 100 | — | — | 91 | 100 |

* **Universal (100):** every parse/streaming operation (DOM and push/pull
  streaming both answer distinct questions, including on the huge tier), every
  XPath operation, every serialization operation.
* **DTD validation (22):** every member carrying a DOCTYPE — JATS ×5,
  MusicXML ×10, SVG ×5, DocBook ×1, and the PubMed baseline. Each entry's
  external DTD system identifier is retained as a recorded dependency
  (`dependencies.dtd`); `svg-006` additionally has an internal subset.
* **XSD validation (10):** the Maven POMs, each a canonical instance of
  `http://maven.apache.org/xsd/maven-4.0.0.xsd` (recorded in
  `resources.xsd_families` and per entry in `dependencies.xsd`). XSD/WSDL
  *definition* documents are deliberately **not** treated as instance targets.
* **XSLT (91):** all families declare a transform (TEI/DocBook use upstream
  stylesheets; the rest a deterministic generic transform fixed before timing).
  The 9 members above `policy.xslt_max_bytes` (64 MiB) — the 8 OSM extracts and
  the PubMed baseline — are excluded because DOM-bound XSLT on a
  multi-hundred-MiB document is not document-appropriate work, not because of
  any candidate result.
* **SimpleXML (91):** capped at `policy.simplexml_max_bytes` (32 MiB) for the
  same reason; `XMLReader` remains the streaming PHP workload for the huge tier.

Every member's `excluded` array names the exact reason for each dropped
`consumer:operation`, so the matrix is fully auditable.

## 4. Freeze and amendment discipline

The file pins its inputs: `input_basis.manifest_sha256`,
`input_basis.report_sha256`, and `input_basis.policy_sha256` (a canonical digest
of the catalog + policy + resource tables + aggregation contract). `--check`
rebuilds the matrix from the current manifest/report and compares it
field-by-field with the committed file; any mismatch is a hard failure:

```
FAIL: eligibility drift — recompute with --amend "<rationale>"
```

`--amend "<rationale>"` increments `revision`, refreshes `frozen_at`, and appends
an amendment record carrying the rationale, timestamp, and the manifest/report/
policy hashes at amendment time — the "explicit amendment receipt with rationale"
required once final measurement begins.

**Amendment 1 → revision 2 (pre-16.12).** The post-review corrections recorded
in `16-10-corpus.md` §9 (renamed/quantified characterization, report↔manifest
binding) changed the
manifest and report digests, and this phase adds the §16.12 aggregation contract.
Because measurement has not begun, the change is recorded as an explicit
amendment rather than a silent recompute; the amendment record carries the full
rationale and the input hashes. No eligibility outcome changed.

## 5. Aggregation contract (§16.12 constraint, frozen here)

Because the population is strongly skewed (26/37/25/3/4/5 across the six size
buckets), a single unstratified “overall × faster” number is a misleading
headline: a per-file mean over-represents tiny XML, a byte-weighted mean is
dominated by the 8.41 GiB tail, and neither answers the same question. The
frozen `aggregation_policy` therefore requires that the authoritative 16.12
output include **all** of:

* the complete per-cell distribution (every file × consumer × operation),
* a macro average by file,
* a micro, byte-weighted throughput,
* category-stratified results,
* size-bucket-stratified results,

and forbids presenting a single unstratified overall speedup as the primary
evidence. Result equivalence (§16.14) remains a prerequisite to accepting any
timing.

## 6. Validation

* `python3 tools/bench/corpus_eligibility.py --check` → `eligibility seal: OK
  (revision 2, 100 files)`.
* The amendment path was exercised (revision 2 published an amendment record and
  passed `--check`).
* CI job `§16.10 real-world XML corpus seal` now runs both `corpus_seal.py` and
  `corpus_eligibility.py --check`.

## 7. Next phase

*16.12* builds the five fixed consumer performance drivers (xmllint, xsltproc,
python3-lxml, ruby-nokogiri, php) and, for the `(consumer, operation)` pairs this
matrix permits, records timings — with result equivalence (§16.14) as a
prerequisite to any timing being accepted.
