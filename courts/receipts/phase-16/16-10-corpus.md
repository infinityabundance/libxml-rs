# Phase 16.10 — 100-file real-world XML performance corpus: research receipt

Commit: (this commit)
Date: 2026-09-13
Phase: 16.10 (§16.10.1–§16.10.6). Candidate SHA at acquisition: `8744adc1`.
Host: AMD Ryzen 7 9800X3D (8C/16T), 9.0 GB of XML acquired (8.41 GiB).

Raw evidence:

| Artifact | Path |
|---|---|
| Corpus of record (100 entries, hashes, metadata) | `tools/bench/corpus/manifest.json` |
| Generated metadata + diversity report | `courts/receipts/phase-16/corpus-report.json` |
| Approved-source fetcher | `tools/bench/fetch_corpus.py` |
| Curation driver (approved sources + pinned revisions) | `tools/bench/corpus/build_manifest.py` |
| Provider-neutral analyzer + seal | `tools/bench/corpus_report.py` |
| Cache-independent seal gate | `tools/bench/corpus_seal.py` |

**Headline result.** The corpus is complete: **exactly 100 naturally occurring,
publicly obtainable, licence-recorded, SHA-256-pinned real-world XML files**
spanning **8.41 GiB**, the exact §16.10.2 category distribution, and **all 14
§16.10.6 composition dimensions present** (`corpus_seal.py` → `SEAL OK`). The
seal is a real gate: a missing major dimension, a malformed member, a missing
hash, or a category-count deviation fails it non-zero.

Reproduce (byte-for-byte; the cache is gitignored and may live outside the repo):

```sh
python3 tools/bench/corpus/build_manifest.py
LIBXML_RS_CORPUS_CACHE=/tmp/lxmlrs-corpus python3 tools/bench/fetch_corpus.py --all
LIBXML_RS_CORPUS_CACHE=/tmp/lxmlrs-corpus python3 tools/bench/corpus_report.py --update-manifest --strict
python3 tools/bench/corpus_seal.py
```

---

## 1. §16.10.1 — corpus rules

Every member is naturally occurring XML used by a real project/dataset/system,
retrieved from an approved source at a pinned revision (git commit/tag, Maven
GA coordinate, or dated snapshot) and verified by SHA-256. **No file was
generated, padded, repeated, sliced, or structurally modified.** No file was
selected because a candidate measurement looked good. A whole OSM extract counts
as one file; the whole PubMed baseline counts as one file.

Retrieval discipline (`fetch_corpus.py`): approved sources only, polite
`User-Agent`, per-host rate limiting (`rate_limit_ms`, default 1000 ms),
streamed downloads, SHA-256 of the compressed **and** decoded bytes, and a
**hard failure on any hash/provenance change — the fetcher never silently
replaces a changed file**. The corpus is stored in a gitignored cache
(`downloads/`, `files/`, `state.json`); redistribution stays off until an entry
is explicitly marked `redistributable` + `redistribution_verified` (all 100
entries are currently `false`, so no bytes are redistributed from the repo).

## 2. §16.10.2 — exact distribution

| Category | Count | Source(s) |
|---|---|---|
| PMC Open Access JATS | 10 | Europe PMC REST `fullTextXML` |
| SEC EDGAR XML/XBRL | 8 | `dgunning/edgartools` (see §2.1) |
| OpenStreetMap extracts | 8 | BBBike city extracts (see §2.3) |
| Monthly bibliographic snapshot | 1 | NLM PubMed baseline (see §2.2) |
| AOSP Android XML | 10 | `android.googlesource.com` gitiles @ `android-14.0.0_r1` |
| Maven POM | 10 | Maven Central (`repo1.maven.org`) |
| TEI | 10 | `TEIC/TEI`, `TEIC/Stylesheets` |
| MusicXML | 10 | `cuthbertLab/music21`, `CPJKU/partitura` |
| SVG | 8 | `linebender/resvg`, `apache/xmlgraphics-batik` |
| DocBook | 8 | `docbook/xslTNG`, `docbook/xslt10-stylesheets` |
| GPX/KML | 7 | OSGeo GDAL, libkml, GPSBabel |
| RSS/Atom | 5 | W3C feedvalidator, `kurtmckee/feedparser` |
| SOAP/WSDL/XSD | 5 | Spring WS, Apache CXF, W3C `xsdtests` |
| **Total** | **100** | |

### 2.1 SEC/XBRL substitution (documented)

Direct SEC EDGAR hosts (`www.sec.gov`, `data.sec.gov`) return **HTTP 403** from
the retrieval host. Real EDGAR/XBRL filing instances are therefore retrieved
from the MIT-licensed **`dgunning/edgartools`** repository (pinned
`abe44344c56cf4bfb5443e0debca7e39342f6e7a`), which mirrors genuine SEC filings.
The eight instances span 20 KB–8.85 MB and preserve the required structure:
namespace-heavy XBRL instances (`xbrli`/`us-gaap`/`dei`/`xbrldi`), linkbases,
inline-XBRL, and form-level documents (NPORT, 13F, 8-K, 10-K).

### 2.2 DBLP substitution (documented)

Persistent monthly DBLP snapshots (`dblp.org/xml/release/`) are unavailable from
the retrieval host: the release directory is behind an anti-bot challenge and a
direct dump URL returns a small HTML page, not the dataset. The single
very-large DTD-bearing **bibliographic** dataset is therefore the NLM **PubMed
baseline** `pubmed26n0001.xml.gz` (2026 baseline; 19.7 MB gzip → 195 MB XML,
real `<!DOCTYPE ... SYSTEM ...>` DTD). It fills the same structural role —
massive repetitive real-world bibliographic records with a DTD — and, like a
DBLP snapshot, counts as exactly **one** file.

### 2.3 OSM substitution (documented)

`download.geofabrik.de` `.osm.bz2` extracts return **404** from the retrieval
host. BBBike city extracts are used instead. BBBike files are daily-rolling and
have no immutable revision, so each is pinned by **SHA-256 + retrieval date**
(`BBBike:<City>@retrieved-2026-09-13`); a changed source fails the fetcher
rather than silently replacing the file. Attribution `© OpenStreetMap
contributors` and **ODbL-1.0** are recorded per file.

The eight extracts were chosen to span the acceleration-crossover range and
script diversity:

| ID | City | Script | Bytes (uncompressed) | Bucket |
|---|---|---|---|---|
| osm-001 | Baghdad | Arabic | 167,944,732 | 32–256 MiB |
| osm-002 | Cairo | Arabic | 797,144,886 | >256 MiB |
| osm-003 | Beirut | Arabic | 177,651,387 | 32–256 MiB |
| osm-004 | Phnom Penh | Khmer | 87,696,466 | 32–256 MiB |
| osm-005 | Bangkok | Thai | 1,028,793,700 | >256 MiB |
| osm-006 | Moscow | Cyrillic | 1,440,317,789 | >256 MiB |
| osm-007 | Tokyo | CJK | 2,341,347,891 | >256 MiB |
| osm-008 | Warsaw | Latin (PL) | 2,738,954,768 | >256 MiB |

## 3. §16.10.3 — approved-source discipline and licences

Approved retrieval services are used throughout: the **Europe PMC REST API**
(not HTML scraping), Maven Central, gitiles, `raw.githubusercontent.com`, BBBike,
and the NLM FTP mirror. No bulk PMC scraping occurs.

PMC has per-article licence variation, so each article's applicable licence is
read from its own `<permissions><license>` / `<ali:license_ref>` and recorded in
`license`, `spdx`, `license_url` and `license_note`. The ten articles resolve to
**CC BY 4.0 (×6), CC BY-NC 3.0 (×3), CC BY-NC-SA 4.0 (×1)**.

Per-source SPDX distribution across the corpus:

| SPDX | Files | SPDX | Files |
|---|---|---|---|
| Apache-2.0 | 30 | BSD-3-Clause | 11 |
| MIT | 20 | CC-BY-4.0 | 6 |
| BSD-2-Clause | 13 | ODbL-1.0 | 8 |
| CC-BY-NC-3.0 | 3 | GPL-2.0-or-later | 3 |
| CC-BY-NC-SA-4.0 | 1 | EPL-2.0 | 2 |
| W3C | 1 | `LicenseRef-DocBook-XSL-Stylesheets` | 1 |
| `LicenseRef-NLM-PubMed` | 1 | | |

Every entry records `attribution` and `license_url`. The three
`GPL-2.0-or-later` GPSBabel files are legal for benchmark **use**; because bytes
are not redistributed (cache-only, `redistributable=false`) no copyleft
obligations attach to the repository. The two `LicenseRef-*` values are used
where no exact SPDX identifier is accurate (NLM public-domain data and the
DocBook XSL MIT-style custom licence).

## 4. §16.10.4 — manifest and fetcher

`tools/bench/corpus/manifest.json` has one entry per file with every required
field: stable id, filename, category, source organisation, source URL/API id,
exact release/commit/date (`source_ref` + `retrieved_at`), retrieval method,
licence/SPDX/licence URL/attribution, compressed + uncompressed SHA-256, bytes,
encoding, XML version, DOCTYPE/internal/external-subset presence, namespace
count, distinct element/attribute names, total elements/attributes, max depth,
text-byte fraction, attribute-byte fraction, non-ASCII fraction, entity-ref
count, comments, CDATA, PIs, schema/DTD dependencies, suitability labels, and
consumer eligibility. The benchmark works from the manifest.

The statistics are extracted by a **provider-neutral streaming parser** —
Python's Expat, deliberately not the candidate under test — so the corpus
metadata is independent of the implementation being benchmarked. Analysis
streams gigabyte-scale files and classifies scripts in both character data and
attribute values (OSM/XBRL/SVG carry Unicode in attributes).

## 5. §16.10.5 — size distribution (actual vs target)

| Bucket | Actual | Target | Note |
|---|---|---|---|
| `<16 KiB` | 26 | 20 | |
| `16 KiB–256 KiB` | 37 | 25 | |
| `256 KiB–4 MiB` | **25** | 25 | exact |
| `4–32 MiB` | 3 | 15 | short (see below) |
| `32–256 MiB` | 4 | 10 | short (see below) |
| `>256 MiB` | **5** | 5 | exact |

**Documented deviation.** Single naturally occurring XML documents in the
`4–32 MiB` and `32–256 MiB` ranges are genuinely scarce across the fixed
§16.10.2 categories: TEI/MusicXML/DocBook/SVG/JATS/AOSP/Maven max out in the
single-digit-MiB range, and only OSM/PubMed reached beyond it. Rather than pad,
generate, concatenate, or slice (all forbidden), the shortfall is recorded here
and structural diversity is preserved. The very-large tier — the one the spec
calls out for Rayon/AVX-512/CUDA crossover — is hit **exactly (5)**, including
1.03 GB, 1.44 GB, 2.34 GB and 2.74 GB inputs.

## 6. §16.10.6 — composition diversity

`corpus_report.py --strict` generates `courts/receipts/phase-16/corpus-report.json`
and fails the seal if any major dimension is absent. All 14 required dimensions
are present:

| Dimension | Files | Dimension | Files |
|---|---|---|---|
| predominantly ASCII | 73 | broad trees | 20 |
| Western Latin Unicode | 42 | DTD/entity documents | 22 |
| non-Latin scripts | 35 | CDATA/comments/PIs | 67 |
| mixed-script Unicode | 32 | tiny configuration XML | 26 |
| markup-heavy | 82 | huge dataset XML | 9 |
| text-heavy | 27 | namespace-heavy | 79 |
| attribute-heavy | 35 | deep trees | 22 |

CJK appears (TEI + OSM Tokyo), as do Arabic, Cyrillic, Thai, Khmer and Devanagari
script content. `absent_required_dimensions: []`.

## 7. Limitations and risks

* **BBBike OSM extracts are daily-rolling.** There is no immutable OSM revision
  available from the approved hosts, so OSM members are pinned by SHA-256 +
  retrieval date. The fetcher fails loudly if the live file ever differs.
* **SEC/XBRL and DBLP substitutions** are documented above; both preserve the
  required structural role and are pinned to immutable repository revisions.
* **Bytes are not redistributed.** The 8.41 GiB corpus lives in a gitignored
  cache (`LIBXML_RS_CORPUS_CACHE`, default `tools/bench/corpus/`). Committed
  evidence is the manifest, the report, and the tooling.
* The `4–32 MiB` and `32–256 MiB` buckets are under target for the natural-
  availability reasons in §5; this is reported, not faked.

## 8. Next phase

*16.11* freezes consumer eligibility into
`courts/receipts/phase-16/corpus-eligibility.json`, derived from the manifest's
`consumer_eligibility`/`suitability` labels. The corpus stays separate from the
correctness/oracle corpus.
