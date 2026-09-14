#!/usr/bin/env python3
"""corpus_report.py — §16.10 metadata, diversity report and seal.

Reads the fetched corpus (`tools/bench/corpus/files/`), extracts the metadata
required by §16.10.4 with a **provider-neutral streaming parser** (Python's
Expat — not the candidate under test), merges it into `manifest.json`, and
enforces the §16.10.5/§16.10.6 seals:

* the size distribution is reported against the §16.10.5 target buckets;
* the §16.10.6 composition dimensions are checked and a missing *major*
  dimension fails the seal (exit non-zero).

Outputs:
  tools/bench/corpus/manifest.json                 (enriched, in place)
  courts/receipts/phase-16/corpus-report.json      (report of record)

Usage:
  python3 tools/bench/corpus_report.py --update-manifest
  python3 tools/bench/corpus_report.py --update-manifest --strict
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import xml.parsers.expat

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CORPUS = os.path.join(ROOT, "tools", "bench", "corpus")
MANIFEST = os.path.join(CORPUS, "manifest.json")
CACHE = os.environ.get("LIBXML_RS_CORPUS_CACHE", CORPUS)
FILES = os.path.join(CACHE, "files")
REPORT = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-report.json")

# §16.10.5 target buckets (upper bounds, bytes).
BUCKETS = [
    ("t0_lt_16k", 0, 16 * 1024, 20),
    ("t1_16k_256k", 16 * 1024, 256 * 1024, 25),
    ("t2_256k_4m", 256 * 1024, 4 * 1024 * 1024, 25),
    ("t3_4m_32m", 4 * 1024 * 1024, 32 * 1024 * 1024, 15),
    ("t4_32m_256m", 32 * 1024 * 1024, 256 * 1024 * 1024, 10),
    ("t5_gt_256m", 256 * 1024 * 1024, None, 5),
]

# §16.10.6 dimensions that MUST be present for the seal.
REQUIRED_DIMENSIONS = [
    "predominantly_ascii",
    "western_latin_unicode",
    "non_latin_script",
    "cjk",
    "mixed_script",
    "markup_heavy",
    "text_heavy",
    "attribute_heavy",
    "namespace_heavy",
    "deep_tree",
    "broad_tree",
    "dtd_or_entity",
    "cdata_or_comment_or_pi",
    "tiny_configuration",
    "huge_dataset",
]

# Keys corpus_report itself derives from the file bytes. Excluded from the
# manifest-core projection so the report's cryptographic binding to the
# manifest is stable regardless of whether enrichment has already run.
ANALYSIS_KEYS = {
    "encoding", "xml_version", "doctype", "internal_subset", "external_subset",
    "namespaces", "distinct_elements", "distinct_attributes", "elements",
    "attributes", "max_depth", "text_fraction", "attribute_fraction",
    "non_ascii_fraction", "entity_declarations", "entity_references",
    "prefixed_element_fraction", "comments", "cdata", "pis", "schema_deps",
    "suitability", "bucket", "parse_ok", "scripts", "entities",
}


def script_class(ch: str) -> str:
    o = ord(ch)
    if o < 0x80:
        return "ascii"
    if o < 0x250:
        return "latin"
    if 0x2E80 <= o <= 0x9FFF or 0x3040 <= o <= 0x30FF or 0xAC00 <= o <= 0xD7AF:
        return "cjk"
    return "other"


_HI = bytes(range(0x80, 0x100))
_PREDEFINED_ENTITIES = {b"amp", b"lt", b"gt", b"quot", b"apos"}
_ENTITY_REF_RE = re.compile(rb"&([A-Za-z_][A-Za-z0-9_.:-]*);")


def byte_stats(path: str) -> tuple[int, int]:
    """Stream the file, returning (total_bytes, non_ascii_bytes) without
    materialising gigabyte-scale files in memory."""
    total = 0
    non_ascii = 0
    with open(path, "rb") as f:
        while True:
            chunk = f.read(1 << 23)
            if not chunk:
                break
            total += len(chunk)
            non_ascii += len(chunk) - len(chunk.translate(None, _HI))
    return total, non_ascii


def count_entity_references(path: str) -> int:
    """Lexically count `&name;` references excluding the five predefined
    entities and numeric character references (`&#...;`).

    Expat expands internal entity references into character data, so the parser
    cannot be used for this; the count is an explicit byte-level scan (a
    reference occurring textually inside a comment or CDATA section is counted
    as the lexical reference it is). Chunk boundaries are handled by carrying a
    short tail and only counting matches that end inside the new region."""
    total = 0
    carry = b""
    with open(path, "rb") as f:
        while True:
            chunk = f.read(1 << 23)
            if not chunk:
                break
            data = carry + chunk
            base = len(carry)
            for m in _ENTITY_REF_RE.finditer(data):
                if m.end() > base and m.group(1) not in _PREDEFINED_ENTITIES:
                    total += 1
            carry = data[-64:]
    return total


def utf8len(s: str) -> int:
    return len(s) if s.isascii() else len(s.encode("utf-8", "replace"))


def add_scripts(scripts: set[str], s: str) -> None:
    """Record non-ASCII scripts present in text content or attribute values
    (OSM, SVG, XBRL and many config formats carry their Unicode in attributes)."""
    if s.isascii():
        return
    for c in s:
        sc = script_class(c)
        if sc != "ascii":
            scripts.add(sc)


def analyze(path: str) -> dict:
    total, non_ascii = byte_stats(path)

    st = {
        "elements": 0,
        "attributes": 0,
        "max_depth": 0,
        "text_bytes": 0,
        "attr_value_bytes": 0,
        "entity_declarations": 0,
        "prefixed_elements": 0,
        "comments": 0,
        "cdata": 0,
        "pis": 0,
        "doctype": False,
        "internal_subset": False,
        "external_subset": False,
        "namespaces": 0,
        "xml_version": None,
        "encoding": None,
    }
    elem_names: set[str] = set()
    attr_names: set[str] = set()
    scripts: set[str] = set()
    schema_deps: set[str] = set()

    p = xml.parsers.expat.ParserCreate()

    def xml_decl(version, encoding, standalone):
        st["xml_version"] = version
        st["encoding"] = encoding

    def doctype(name, sysid, pubid, has_internal):
        st["doctype"] = True
        st["internal_subset"] = bool(has_internal)
        if sysid or pubid:
            st["external_subset"] = True
            schema_deps.add(sysid or pubid)

    stack: list[str] = []

    def start(name, attrs):
        st["elements"] += 1
        elem_names.add(name)
        if ":" in name:
            st["prefixed_elements"] += 1
        depth = len(stack) + 1
        if depth > st["max_depth"]:
            st["max_depth"] = depth
        items = attrs.items() if attrs else ()
        for k, v in items:
            st["attributes"] += 1
            attr_names.add(k)
            if k == "xmlns" or k.startswith("xmlns:"):
                st["namespaces"] += 1
            if not v:
                continue
            st["attr_value_bytes"] += utf8len(v)
            add_scripts(scripts, v)
            if k.endswith("schemaLocation") or k.endswith("noNamespaceSchemaLocation"):
                for part in v.split():
                    if "/" in part or part.endswith((".xsd", ".dtd", ".rng")):
                        schema_deps.add(part)
        stack.append(name)

    def start_ns(prefix, uri):
        st["namespaces"] += 1

    def end(_name):
        if stack:
            stack.pop()

    def chardata(data):
        st["text_bytes"] += utf8len(data)
        add_scripts(scripts, data)

    def comment(_data):
        st["comments"] += 1

    def cdata_start():
        st["cdata"] += 1

    def pi(_t, _d):
        st["pis"] += 1

    def entity(*_a):
        st["entity_declarations"] += 1

    p.XmlDeclHandler = xml_decl
    p.StartDoctypeDeclHandler = doctype
    p.StartElementHandler = start
    p.EndElementHandler = end
    p.StartNamespaceDeclHandler = start_ns
    p.CharacterDataHandler = chardata
    p.CommentHandler = comment
    p.StartCdataSectionHandler = cdata_start
    p.ProcessingInstructionHandler = pi
    p.EntityDeclHandler = entity
    p.UnparsedEntityDeclHandler = entity
    p.ExternalEntityRefHandler = lambda *a: 1  # don't fetch external resources
    try:
        p.SetParamEntityParsing(xml.parsers.expat.XML_PARAM_ENTITY_PARSING_NEVER)
    except Exception:  # noqa: BLE001
        pass

    parse_ok = True
    parse_error = None
    try:
        with open(path, "rb") as f:
            while True:
                chunk = f.read(1 << 22)
                if not chunk:
                    break
                p.Parse(chunk, False)
            p.Parse(b"", True)
    except xml.parsers.expat.ExpatError as e:
        parse_ok = False
        parse_error = str(e)

    return {
        "bytes": total,
        "non_ascii_fraction": round(non_ascii / total, 6) if total else 0.0,
        "elements": st["elements"],
        "attributes": st["attributes"],
        "distinct_elements": len(elem_names),
        "distinct_attributes": len(attr_names),
        "max_depth": st["max_depth"],
        "text_fraction": round(st["text_bytes"] / total, 6) if total else 0.0,
        "attribute_fraction": round(st["attr_value_bytes"] / total, 6) if total else 0.0,
        "namespaces": st["namespaces"],
        "prefixed_element_fraction": (round(st["prefixed_elements"] / st["elements"], 6)
                                      if st["elements"] else 0.0),
        "entity_declarations": st["entity_declarations"],
        "entity_references": count_entity_references(path),
        "comments": st["comments"],
        "cdata": st["cdata"],
        "pis": st["pis"],
        "doctype": st["doctype"],
        "internal_subset": st["internal_subset"],
        "external_subset": st["external_subset"],
        "xml_version": st["xml_version"] or "1.0",
        "encoding": st["encoding"] or "UTF-8",
        "scripts": sorted(scripts),
        "schema_deps": sorted(schema_deps),
        "parse_ok": parse_ok,
        "parse_error": parse_error,
    }


def suitability(m: dict) -> list[str]:
    s = []
    if m["non_ascii_fraction"] < 0.001:
        s.append("predominantly_ascii")
    if "latin" in m["scripts"]:
        s.append("western_latin_unicode")
    if "other" in m["scripts"]:
        s.append("non_latin_script")
    if "cjk" in m["scripts"]:
        s.append("cjk")
    if len(m["scripts"]) >= 2:
        s.append("mixed_script")
    ratio = m["elements"] / max(1, m["bytes"])
    if ratio > 5e-3:
        s.append("markup_heavy")
    if m["text_fraction"] > 0.5:
        s.append("text_heavy")
    if m["attributes"] and m["attribute_fraction"] > 0.15:
        s.append("attribute_heavy")
    if m["namespaces"]:
        s.append("has_namespace_declarations")
    # namespace-heavy is quantitative: most elements are namespace-qualified.
    if m["namespaces"] and m["prefixed_element_fraction"] >= 0.5:
        s.append("namespace_heavy")
    if m["max_depth"] >= 10:
        s.append("deep_tree")
    if m["elements"] >= 10000 and m["max_depth"] <= 8:
        s.append("broad_tree")
    if m["doctype"] or m["entity_declarations"] or m["entity_references"]:
        s.append("dtd_or_entity")
    if m["cdata"] or m["comments"] or m["pis"]:
        s.append("cdata_or_comment_or_pi")
    if m["bytes"] < 16 * 1024:
        s.append("tiny_configuration")
    if m["bytes"] >= 32 * 1024 * 1024:
        s.append("huge_dataset")
    return s


def bucket_of(nbytes: int) -> str:
    for name, lo, hi, _target in BUCKETS:
        if nbytes >= lo and (hi is None or nbytes < hi):
            return name
    return BUCKETS[-1][0]


def manifest_core(doc: dict) -> dict:
    """The provenance projection of a manifest entry: everything except the
    characterization keys that corpus_report itself derives. Hashing this makes
    the report's binding to the manifest stable across enrichment order."""
    entries = [{k: v for k, v in e.items() if k not in ANALYSIS_KEYS}
               for e in doc["entries"]]
    return {"schema": doc.get("schema"), "phase": doc.get("phase"),
            "retrieved_at": doc.get("retrieved_at"), "entries": entries}


def manifest_core_sha256(doc: dict) -> str:
    payload = json.dumps(manifest_core(doc), sort_keys=True,
                         separators=(",", ":"), ensure_ascii=False).encode()
    return hashlib.sha256(payload).hexdigest()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--update-manifest", action="store_true")
    ap.add_argument("--strict", action="store_true",
                    help="fail when a required diversity dimension is absent")
    args = ap.parse_args()

    with open(MANIFEST, "r", encoding="utf-8") as f:
        doc = json.load(f)
    entries = doc["entries"]
    core_sha = manifest_core_sha256(doc)

    report_rows = []
    missing = []
    for e in entries:
        path = os.path.join(FILES, e["filename"])
        if not os.path.exists(path):
            missing.append(e["id"])
            continue
        m = analyze(path)
        m["bucket"] = bucket_of(m["bytes"])
        m["suitability"] = suitability(m)
        e.pop("entities", None)  # legacy name for entity_declarations
        e.update({k: m[k] for k in (
            "bytes", "encoding", "xml_version", "doctype", "internal_subset",
            "external_subset", "namespaces", "distinct_elements", "distinct_attributes",
            "elements", "attributes", "max_depth", "text_fraction",
            "attribute_fraction", "non_ascii_fraction", "entity_declarations",
            "entity_references", "prefixed_element_fraction", "comments",
            "cdata", "pis", "schema_deps", "suitability")})
        e["bucket"] = m["bucket"]
        e["parse_ok"] = m["parse_ok"]
        e["scripts"] = m["scripts"]
        if not m["parse_ok"]:
            e["suitability"].append("well-formedness_gap")
        report_rows.append({"id": e["id"], "category": e["category"],
                            "filename": e["filename"],
                            "uncompressed_sha256": e.get("uncompressed_sha256"),
                            **m})

    bucket_counts = {name: 0 for name, *_ in BUCKETS}
    for r in report_rows:
        bucket_counts[r["bucket"]] += 1

    dims: dict[str, int] = {d: 0 for d in REQUIRED_DIMENSIONS}
    for r in report_rows:
        for d in r["suitability"]:
            if d in dims:
                dims[d] += 1

    absent = [d for d in REQUIRED_DIMENSIONS if dims[d] == 0]
    report = {
        "schema": "corpus-report/1",
        "phase": "16.10",
        "manifest_core_sha256": core_sha,
        "manifest_core_projection": "manifest entries excluding ANALYSIS_KEYS "
                                    "(report-derived characterization fields)",
        "total_files": len(entries),
        "fetched_files": len(report_rows),
        "missing_files": missing,
        "categories": {
            c: sum(1 for e in entries if e["category"] == c)
            for c in sorted({e["category"] for e in entries})
        },
        "size_buckets": {
            name: {"count": bucket_counts[name], "target": target}
            for name, _lo, _hi, target in BUCKETS
        },
        "diversity_dimensions": dims,
        "absent_required_dimensions": absent,
        "well_formedness_gaps": [r["id"] for r in report_rows if not r["parse_ok"]],
        "rows": report_rows,
    }

    os.makedirs(os.path.dirname(REPORT), exist_ok=True)
    with open(REPORT, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=1, ensure_ascii=False)
        f.write("\n")

    if args.update_manifest:
        with open(MANIFEST, "w", encoding="utf-8") as f:
            json.dump(doc, f, indent=1, ensure_ascii=False)
            f.write("\n")

    print(f"corpus: {len(report_rows)}/{len(entries)} files fetched; missing={len(missing)}")
    for name, _lo, _hi, target in BUCKETS:
        print(f"  {name:14} {bucket_counts[name]:3} (target {target})")
    print("diversity:")
    for d in REQUIRED_DIMENSIONS:
        mark = "ok " if dims[d] else "ABSENT"
        print(f"  {mark} {d}: {dims[d]}")
    if absent:
        print(f"absent required dimensions: {absent}")
        if args.strict:
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
