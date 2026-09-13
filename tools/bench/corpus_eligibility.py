#!/usr/bin/env python3
"""corpus_eligibility.py — §16.11 freeze consumer eligibility before timing.

Each of the 100 corpus files receives an **eligibility matrix** — the set of
(consumer, operation) pairs it may enter — computed **before any candidate
performance result is inspected**, and stored at:

    courts/receipts/phase-16/corpus-eligibility.json

Eligibility is a function of exactly five inputs, per the spec:

* consumer API support        — the fixed operation catalog per consumer,
* semantic appropriateness     — e.g. no validation without a target, no
                                 SimpleXML/XSLT on absurdly large documents,
* document dependencies        — the manifest's DOCTYPE / schemaLocation deps,
* data format                  — all members are XML (RDF/XML included),
* available stylesheet/schema/query — the per-family resource declaration.

It deliberately reads **only** `tools/bench/corpus/manifest.json` and
`courts/receipts/phase-16/corpus-report.json`. It never reads timing data, so a
file can never be excluded because the candidate loses on it (§16.11).

Freeze discipline: the committed file records the SHA-256 of the manifest, the
report and the policy it was computed from. `--check` recomputes and fails
closed on any drift. After measurement begins, changing eligibility requires an
explicit amendment: `--amend "<rationale>"`, which bumps the revision and
appends an amendment receipt. `--check` is wired into CI.

Usage:
  python3 tools/bench/corpus_eligibility.py                 # generate/refresh
  python3 tools/bench/corpus_eligibility.py --check         # verify frozen seal
  python3 tools/bench/corpus_eligibility.py --amend "why"   # explicit amendment
  python3 tools/bench/corpus_eligibility.py --summary
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CORPUS = os.path.join(ROOT, "tools", "bench", "corpus")
MANIFEST = os.path.join(CORPUS, "manifest.json")
REPORT = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-report.json")
OUT = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-eligibility.json")

CONSUMERS = ["xmllint", "xsltproc", "python3-lxml", "ruby-nokogiri", "php"]

# The fixed operation catalog per consumer (§16.12).
OPERATIONS = {
    "xmllint": ["parse", "stream", "xpath", "dtd_validate", "xsd_validate",
                "relaxng_validate", "serialize"],
    "xsltproc": ["transform", "stylesheet_compile", "precompiled_apply"],
    "python3-lxml": ["dom_parse", "iterparse", "xpath_adhoc", "xpath_compiled",
                     "xpath_compile", "tostring", "xslt", "dtd_validate",
                     "xsd_validate"],
    "ruby-nokogiri": ["dom_parse", "sax_parse", "reader", "xpath", "serialize",
                      "xsd_validate", "xslt"],
    "php": ["dom", "xmlreader", "simplexml", "domxpath", "xsltprocessor",
            "serialize"],
}

# Operations that are always applicable to any well-formed XML member.
ALWAYS = {
    "xmllint": ["parse", "stream", "serialize"],
    "python3-lxml": ["dom_parse", "iterparse", "tostring"],
    "ruby-nokogiri": ["dom_parse", "sax_parse", "reader", "serialize"],
    "php": ["dom", "xmlreader", "serialize"],
}
# XPath needs a meaningful predeclared expression; every family declares one,
# so the xpath class is universally applicable (exact expressions: §16.13).
XPATH = {
    "xmllint": ["xpath"],
    "python3-lxml": ["xpath_adhoc", "xpath_compiled", "xpath_compile"],
    "ruby-nokogiri": ["xpath"],
    "php": ["domxpath"],
}
DTD = {"xmllint": ["dtd_validate"], "python3-lxml": ["dtd_validate"]}
XSD = {"xmllint": ["xsd_validate"], "python3-lxml": ["xsd_validate"],
       "ruby-nokogiri": ["xsd_validate"]}
XSLT = {"xsltproc": ["transform", "stylesheet_compile", "precompiled_apply"],
        "python3-lxml": ["xslt"], "ruby-nokogiri": ["xslt"],
        "php": ["xsltprocessor"]}
SIMPLEXML = {"php": ["simplexml"]}

# Semantic-appropriateness caps (bytes). These are resource/format judgements
# made before timing, identical for oracle and candidate — never a candidate
# score. DOM/streaming parsing stays eligible for every size (the huge tier is
# exactly what DOM vs streaming is meant to contrast); only the two workloads
# that cannot sensibly run on multi-hundred-megabyte documents are capped.
POLICY = {
    "xslt_max_bytes": 64 * 1024 * 1024,
    "simplexml_max_bytes": 32 * 1024 * 1024,
    "xslt_max_reason": "DOM-bound XSLT on a multi-hundred-MiB document is not "
                       "document-appropriate work",
    "simplexml_max_reason": "SimpleXML builds a full PHP tree; multi-hundred-MiB "
                            "documents are not memory-appropriate",
}

# Per-family resource declaration: available queries and transform (§16.13
# materialises the concrete expressions/stylesheets; §16.11 freezes availability).
FAMILY_RESOURCES = {
    "JATS-PMC": {
        "xpath": ["article_title", "authors", "references", "section_count",
                  "descendant_text"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "jats-extract-metadata"}},
    "SEC-XBRL": {
        "xpath": ["facts_by_namespace_localname", "contexts", "units",
                  "numeric_facts"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "xbrl-facts-extract"}},
    "OSM": {
        "xpath": ["nodes", "ways", "tags_by_key", "relation_members"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "osm-node-summary"}},
    "DBLP": {
        "xpath": ["publication_counts_by_type", "title_select", "author_select"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "pubmed-record-summary"}},
    "AOSP": {
        "xpath": ["namespace_qualified_attributes",
                  "activity_service_permission"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "android-permissions-extract"}},
    "MAVEN": {
        "xpath": ["dependencies", "plugins", "properties"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "maven-dependencies-extract"}},
    "TEI": {
        "xpath": ["sections", "paragraphs", "ids_xrefs"],
        "xslt": {"available": True, "kind": "upstream",
                 "name": "TEIC/Stylesheets",
                 "ref": "TEIC/Stylesheets@cda7f87ead3d52629556b1d24726ffc1d042be61"}},
    "DOCBOOK": {
        "xpath": ["sections", "paragraphs", "ids_xrefs"],
        "xslt": {"available": True, "kind": "upstream",
                 "name": "docbook/xslt10-stylesheets",
                 "ref": "docbook/xslt10-stylesheets@efd62655c11cc8773708df7a843613fa1e932bf8"}},
    "MUSICXML": {
        "xpath": ["measures", "notes", "pitch_descendants"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "musicxml-structure-extract"}},
    "SVG": {
        "xpath": ["paths", "groups", "selected_attributes"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "svg-attribute-transform"}},
    "GPX-KML": {
        "xpath": ["trkpt", "placemarks", "coordinates"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "gpx-track-summary"}},
    "RSS-ATOM": {
        "xpath": ["items_entries", "titles", "links"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "feed-items-summary"}},
    "SOAP-WSDL-XSD": {
        "xpath": ["porttype_operations", "schema_elements", "messages"],
        "xslt": {"available": True, "kind": "generic-fixed",
                 "name": "wsdl-operations-extract"}},
}

# Families with a real, available *instance* schema target (the document is an
# instance of a declared schema). XSD/WSDL definition documents are not
# themselves an instance-validation target, so they are not listed here.
XSD_FAMILIES = {
    "MAVEN": {"schema": "http://maven.apache.org/xsd/maven-4.0.0.xsd",
              "target": "instance", "root": "project"},
}

# Reason strings for the (consumer, operation) pairs dropped per file.
REASON = {
    "dtd_validate": "no DOCTYPE present",
    "xsd_validate": "no available instance schema target for this family",
    "relaxng_validate": "no RelaxNG schema resource available",
    "xslt": "no declared transform for family",
    "transform": "no declared transform for family",
    "stylesheet_compile": "no declared transform for family",
    "precompiled_apply": "no declared transform for family",
    "xsltprocessor": "no declared transform for family",
    "simplexml": "document exceeds POLICY.simplexml_max_bytes",
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_path(path: str) -> str:
    with open(path, "rb") as f:
        return sha256_bytes(f.read())


def catalog() -> dict:
    return OPERATIONS


def policy_digest() -> str:
    payload = json.dumps(
        {"operations": OPERATIONS, "policy": POLICY, "family_resources": FAMILY_RESOURCES,
         "always": ALWAYS, "xpath": XPATH, "dtd": DTD, "xsd": XSD, "xslt": XSLT,
         "simplexml": SIMPLEXML, "xsd_families": XSD_FAMILIES},
        sort_keys=True, separators=(",", ":")).encode()
    return sha256_bytes(payload)


def add(dst: dict, src: dict) -> None:
    for consumer, ops in src.items():
        dst.setdefault(consumer, [])
        dst[consumer].extend(ops)


def compute(manifest: dict) -> dict:
    entries = []
    for e in manifest["entries"]:
        cat = e["category"]
        res = FAMILY_RESOURCES[cat]
        eligible: dict[str, list[str]] = {}
        add(eligible, ALWAYS)
        add(eligible, XPATH)
        if e["doctype"]:
            add(eligible, DTD)
        deps = e.get("schema_deps") or []
        dtd_deps = [d for d in deps if d.lower().endswith((".dtd", ".mod"))]
        xsd_res = XSD_FAMILIES.get(cat)
        xsd_deps = [d for d in deps if d.lower().endswith(".xsd")]
        if xsd_res:
            add(eligible, XSD)
            if xsd_res["schema"] not in xsd_deps:
                xsd_deps = xsd_deps + [xsd_res["schema"]]
        xslt_ok = bool(res["xslt"].get("available")) and e["bytes"] <= POLICY["xslt_max_bytes"]
        if xslt_ok:
            add(eligible, XSLT)
        simple_ok = e["bytes"] <= POLICY["simplexml_max_bytes"]
        if simple_ok:
            add(eligible, SIMPLEXML)

        for c in eligible:
            eligible[c] = sorted(set(eligible[c]))

        excluded = []
        for consumer, ops in OPERATIONS.items():
            for op in ops:
                if op in eligible.get(consumer, []):
                    continue
                if op in ("dtd_validate",):
                    why = REASON["dtd_validate"]
                elif op == "xsd_validate":
                    why = REASON["xsd_validate"]
                elif op == "relaxng_validate":
                    why = REASON["relaxng_validate"]
                elif op in ("transform", "stylesheet_compile", "precompiled_apply",
                            "xslt", "xsltprocessor"):
                    why = (REASON["transform"] if not res["xslt"].get("available")
                           else POLICY["xslt_max_reason"])
                elif op == "simplexml":
                    why = POLICY["simplexml_max_reason"]
                else:
                    why = "operation not applicable"
                excluded.append("%s:%s:%s" % (consumer, op, why))

        entries.append({
            "id": e["id"],
            "category": cat,
            "bytes": e["bytes"],
            "eligible": {c: eligible.get(c, []) for c in CONSUMERS},
            "excluded": sorted(excluded),
            "dependencies": {"dtd": sorted(dtd_deps), "xsd": sorted(xsd_deps)},
        })

    per_op = {c: {op: 0 for op in OPERATIONS[c]} for c in CONSUMERS}
    for ent in entries:
        for c in CONSUMERS:
            for op in ent["eligible"][c]:
                per_op[c][op] += 1

    return {
        "entries": entries,
        "summary": {
            "files": len(entries),
            "per_consumer_operation_counts": per_op,
            "files_with_dtd_validation": sum(
                1 for ent in entries if ent["eligible"]["xmllint"] and
                "dtd_validate" in ent["eligible"]["xmllint"]),
            "files_with_xsd_validation": sum(
                1 for ent in entries if "xsd_validate" in ent["eligible"]["xmllint"]),
            "files_with_xslt": sum(
                1 for ent in entries if "transform" in ent["eligible"]["xsltproc"]),
            "files_with_simplexml": sum(
                1 for ent in entries if "simplexml" in ent["eligible"]["php"]),
        },
    }


def build_doc(manifest: dict, report: dict, prev: dict | None, amendments: list) -> dict:
    computed = compute(manifest)
    return {
        "schema": "corpus-eligibility/1",
        "phase": "16.11",
        "revision": (prev or {}).get("revision", 1),
        "frozen_at": (prev or {}).get("frozen_at")
                     or _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "input_basis": {
            "manifest_sha256": sha256_path(MANIFEST),
            "report_sha256": sha256_path(REPORT),
            "policy_sha256": policy_digest(),
            "performance_data_used": False,
            "note": "Eligibility is computed only from manifest metadata and the "
                    "predeclared resource/policy tables; no candidate timing "
                    "influences it (§16.11).",
        },
        "consumers": CONSUMERS,
        "operations": OPERATIONS,
        "policy": POLICY,
        "resources": {"families": FAMILY_RESOURCES, "xsd_families": XSD_FAMILIES},
        "entries": computed["entries"],
        "summary": computed["summary"],
        "amendments": amendments,
    }


def comparable(doc: dict) -> dict:
    return {
        "input_basis": doc["input_basis"],
        "consumers": doc["consumers"],
        "operations": doc["operations"],
        "policy": doc["policy"],
        "resources": doc["resources"],
        "entries": doc["entries"],
        "summary": doc["summary"],
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--amend", metavar="RATIONALE")
    ap.add_argument("--summary", action="store_true")
    args = ap.parse_args()

    with open(MANIFEST, encoding="utf-8") as f:
        manifest = json.load(f)
    with open(REPORT, encoding="utf-8") as f:
        report = json.load(f)

    if args.check:
        if not os.path.exists(OUT):
            print("FAIL: corpus-eligibility.json is missing")
            return 1
        with open(OUT, encoding="utf-8") as f:
            committed = json.load(f)
        fresh = build_doc(manifest, report, committed, committed.get("amendments", []))
        if comparable(committed) != comparable(fresh):
            print("FAIL: eligibility drift — recompute with --amend \"<rationale>\"")
            return 1
        print("eligibility seal: OK (revision %d, %d files)"
              % (committed.get("revision", 1), len(committed["entries"])))
        return 0

    prev = None
    amendments: list = []
    if os.path.exists(OUT):
        with open(OUT, encoding="utf-8") as f:
            prev = json.load(f)
        amendments = prev.get("amendments", [])

    if args.amend:
        if not prev:
            print("FAIL: --amend requires an existing frozen eligibility file")
            return 2
        amendments = amendments + [{
            "revision": prev.get("revision", 1) + 1,
            "at": _dt.datetime.now(_dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "rationale": args.amend,
            "manifest_sha256": sha256_path(MANIFEST),
            "report_sha256": sha256_path(REPORT),
            "policy_sha256": policy_digest(),
        }]
        prev["revision"] = prev.get("revision", 1) + 1
        prev["frozen_at"] = None  # refreshed on rewrite

    doc = build_doc(manifest, report, prev, amendments)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")

    s = doc["summary"]
    print("wrote %s" % OUT)
    print("  files=%d dtd=%d xsd=%d xslt=%d simplexml=%d"
          % (s["files"], s["files_with_dtd_validation"],
             s["files_with_xsd_validation"], s["files_with_xslt"],
             s["files_with_simplexml"]))
    if args.summary:
        for c in CONSUMERS:
            print(" ", c, doc["summary"]["per_consumer_operation_counts"][c])
    return 0


if __name__ == "__main__":
    sys.exit(main())
