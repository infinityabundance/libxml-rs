#!/usr/bin/env python3
"""corpus_seal.py — verify the §16.10 real-world XML performance corpus seal.

This gate does **not** need the 9 GiB file cache: it validates the committed
artifacts of record — `tools/bench/corpus/manifest.json` and
`courts/receipts/phase-16/corpus-report.json` — against the §16.10 contract.

Checks (all must pass; exit non-zero otherwise):

1. the manifest holds exactly 100 entries with the exact §16.10.2 category
   distribution;
2. every entry records the §16.10.4 provenance fields, including 64-hex
   compressed/uncompressed SHA-256 digests and a positive byte count;
3. the report covers all 100 files with none missing and no malformed
   (well-formedness gap) members;
4. the report's size buckets sum to 100 (the §16.10.5 distribution is reported,
   deviations documented, never padded);
5. **the diversity seal**: every §16.10.6 required dimension is present. A
   missing major dimension fails the seal, with a `--allow-absent` override for
   documenting an intentional, recorded shortfall.

Usage: python3 tools/bench/corpus_seal.py [--allow-absent DIM]...
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CORPUS = os.path.join(ROOT, "tools", "bench", "corpus")
MANIFEST = os.path.join(CORPUS, "manifest.json")
REPORT = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-report.json")

# §16.10.2 exact target distribution.
EXPECTED_CATEGORIES = {
    "JATS-PMC": 10, "SEC-XBRL": 8, "OSM": 8, "DBLP": 1, "AOSP": 10,
    "MAVEN": 10, "TEI": 10, "MUSICXML": 10, "SVG": 8, "DOCBOOK": 8,
    "GPX-KML": 7, "RSS-ATOM": 5, "SOAP-WSDL-XSD": 5,
}

# §16.10.4 required provenance keys (analysis keys are added by corpus_report).
REQUIRED_KEYS = [
    "id", "filename", "category", "source_org", "source_ref", "source_url",
    "retrieval_method", "retrieved_at", "license", "spdx", "license_url",
    "attribution", "transform", "rate_limit_ms", "redistributable",
    "redistribution_verified", "consumer_eligibility",
    "compressed_sha256", "uncompressed_sha256", "bytes", "encoding",
    "xml_version", "doctype", "internal_subset", "external_subset",
    "namespaces", "distinct_elements", "distinct_attributes", "elements",
    "attributes", "max_depth", "text_fraction", "attribute_fraction",
    "non_ascii_fraction", "entities", "comments", "cdata", "pis",
    "schema_deps", "suitability",
]

HEX64 = re.compile(r"^[0-9a-f]{64}$")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--allow-absent", action="append", default=[],
                    help="recorded, intentional diversity-dimension shortfall")
    args = ap.parse_args()

    failures: list[str] = []

    with open(MANIFEST, encoding="utf-8") as f:
        manifest = json.load(f)
    entries = manifest["entries"]

    # 1. count + category distribution
    if len(entries) != 100:
        failures.append("expected 100 manifest entries, found %d" % len(entries))
    got = {}
    for e in entries:
        got[e["category"]] = got.get(e["category"], 0) + 1
    if got != EXPECTED_CATEGORIES:
        failures.append("category distribution %r != %r" % (got, EXPECTED_CATEGORIES))

    # 2. provenance completeness
    for e in entries:
        eid = e.get("id", "?")
        for k in REQUIRED_KEYS:
            if k not in e:
                failures.append("%s: missing field %s" % (eid, k))
        for k in ("compressed_sha256", "uncompressed_sha256"):
            if not HEX64.match(str(e.get(k, ""))):
                failures.append("%s: %s is not a sha256" % (eid, k))
        if not isinstance(e.get("bytes"), int) or e["bytes"] <= 0:
            failures.append("%s: bytes is not positive" % eid)

    ids = {e["id"] for e in entries}
    if len(ids) != len(entries):
        failures.append("duplicate ids in manifest")

    # 2b. per-file licence/attribution and substitution documentation.
    for e in entries:
        eid = e.get("id", "?")
        if not str(e.get("license") or "").strip():
            failures.append("%s: empty licence" % eid)
        if not str(e.get("attribution") or "").strip():
            failures.append("%s: empty attribution" % eid)
        if e["category"] == "JATS-PMC":
            if not e.get("article_pmcid"):
                failures.append("%s: JATS entry lacks article_pmcid" % eid)
            if not str(e.get("license_note") or "").strip():
                failures.append("%s: JATS per-article licence not recorded" % eid)
            if not str(e.get("spdx") or "").startswith("CC-"):
                failures.append("%s: JATS licence not resolved to a CC identifier" % eid)
        if e["category"] == "MAVEN" and not str(e.get("license_note") or "").strip():
            failures.append("%s: Maven per-artifact licence not recorded" % eid)
        if e["category"] in ("SEC-XBRL", "DBLP", "OSM") and not str(e.get("notes") or "").strip():
            failures.append("%s: required provenance substitution is not documented" % eid)

    # 3-4. report coverage and size buckets
    with open(REPORT, encoding="utf-8") as f:
        report = json.load(f)
    if report.get("total_files") != 100 or report.get("fetched_files") != 100:
        failures.append("report does not cover all 100 files: %s/%s" % (
            report.get("fetched_files"), report.get("total_files")))
    if report.get("missing_files"):
        failures.append("report lists missing files: %r" % report["missing_files"])
    if report.get("well_formedness_gaps"):
        failures.append("report lists well-formedness gaps: %r"
                        % report["well_formedness_gaps"])
    row_ids = {r["id"] for r in report.get("rows", [])}
    if row_ids != ids:
        failures.append("report rows do not match manifest ids")
    bucket_total = sum(b["count"] for b in report.get("size_buckets", {}).values())
    if bucket_total != 100:
        failures.append("size buckets sum to %d, not 100" % bucket_total)

    # 5. diversity seal
    absent = report.get("absent_required_dimensions", [])
    unallowed = [d for d in absent if d not in args.allow_absent]
    if unallowed:
        failures.append("ABSENT required diversity dimensions: %r" % unallowed)

    print("corpus seal: %d entries, %d files reported, %d categories"
          % (len(entries), len(row_ids), len(got)))
    print("size buckets:", {k: v["count"] for k, v in report.get("size_buckets", {}).items()})
    dims = report.get("diversity_dimensions", {})
    print("diversity dimensions present:", sum(1 for v in dims.values() if v), "/", len(dims))
    if failures:
        for f in failures:
            print("FAIL:", f)
        return 1
    print("SEAL OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
