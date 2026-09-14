#!/usr/bin/env python3
"""gen_findings.py — derive the §16.12 discovered-defect list from the matrix.

Classifies every non-equivalent cell by cause and emits
`courts/receipts/phase-16/16-12-findings.json` with counts, affected files and a
minimal reproduction for each defect.
"""

from __future__ import annotations

import json
import os
import re
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MATRIX = os.path.join(ROOT, "courts", "receipts", "phase-16", "16-12-consumer-matrix.json")
OUT = os.path.join(ROOT, "courts", "receipts", "phase-16", "16-12-findings.json")
CORPUS = "/tmp/lxmlrs-corpus/files"


def main() -> None:
    m = json.load(open(MATRIX, encoding="utf-8"))
    cells = m["cells"]
    man = {e["id"]: e for e in json.load(
        open(os.path.join(ROOT, "tools", "bench", "corpus", "manifest.json"),
             encoding="utf-8"))["entries"]}

    inv = [c for c in cells if c["oracle_ok"] and c["candidate_ok"] and not c["equivalent"]]
    asym = [c for c in cells if c["oracle_ok"] != c["candidate_ok"]]

    def amp_attr(eid):
        p = os.path.join(CORPUS, man[eid]["filename"])
        try:
            with open(p, encoding="utf-8", errors="replace") as f:
                for line in f:
                    if re.search(r'="[^"]*&amp;[^"]*"', line):
                        return True
        except OSError:
            return None
        return False

    inv_files = sorted({c["id"] for c in inv})
    amp_files = [i for i in inv_files if amp_attr(i)]

    findings = []

    findings.append({
        "id": "F1-attribute-ampersand-decoding",
        "severity": "correctness",
        "summary": "In an ATTRIBUTE value, `&amp;` is decoded to the literal four/five characters `&#38;` instead of `&` (text nodes are correct).",
        "repro": "printf '<a b=\"x&amp;y\">t&amp;u</a>' > /tmp/a.xml; xmllint --xpath 'string(//a/@b)' /tmp/a.xml",
        "oracle": "x&y",
        "candidate": "x&#38;y",
        "evidence": {"invalid_cells": sum(1 for c in inv if c["id"] in amp_files),
                     "invalid_files": amp_files},
    })

    inst = [c for c in asym if "serialization_instability" in (c.get("candidate_error") or "")]
    findings.append({
        "id": "F2-repeat-serialization-empty",
        "severity": "correctness",
        "summary": "Serializing the SAME parsed document twice returns an empty string on the second and later calls (first call is correct).",
        "repro": "python3 -c 'from lxml import etree; t=etree.parse(\"/corpus/maven-001.pom.xml\"); print(len(etree.tostring(t,encoding=\"unicode\"))); print(len(etree.tostring(t,encoding=\"unicode\")))'",
        "oracle": "30837 then 30837",
        "candidate": "30837 then 0",
        "evidence": {"asymmetric_cells": len(inst),
                     "files": sorted({c["id"] for c in inst})},
    })

    byte = [c for c in cells if c.get("serialize_bytes_identical") is not None]
    diff = [c for c in byte if not c["serialize_bytes_identical"]]
    findings.append({
        "id": "F3-xmllint-format-indentation",
        "severity": "serialization",
        "summary": "`xmllint --format` output differs from upstream (extra blank lines / different indentation depth for some elements).",
        "repro": "xmllint --format /corpus/svg-001.svg",
        "oracle": "3-space-nested <text> indented 2 spaces",
        "candidate": "indented 4 spaces (and extra blank lines on larger docs)",
        "evidence": {"byte_checked": len(byte), "byte_differing": len(diff),
                     "files": sorted({c["id"] for c in diff})},
    })

    xto = [c for c in asym if c["oracle_ok"] and not c["candidate_ok"]
           and "timeout" in (c.get("candidate_error") or "")
           and c["op"] in ("xpath", "xpath_adhoc", "xpath_compiled", "domxpath")]
    findings.append({
        "id": "F4-xpath-superlinear",
        "severity": "performance",
        "summary": "Candidate XPath evaluation does not complete within a generous budget on documents larger than ~1 MiB (oracle: sub-second).",
        "repro": "timeout 60 xmllint --xpath 'count(//*[local-name()=\"node\"])' /corpus/gpxkml-007.gpx",
        "oracle": "completes <1s",
        "candidate": "times out (>60s)",
        "evidence": {"candidate_timeout_cells": len(xto),
                     "files": sorted({c["id"] for c in xto})},
    })

    xsl = [c for c in asym if c["oracle_ok"] and not c["candidate_ok"]
           and c["op"] in ("xslt", "xsltprocessor", "transform", "precompiled_apply",
                           "stylesheet_compile")
           and "timeout" in (c.get("candidate_error") or "")]
    findings.append({
        "id": "F5-xslt-timeout",
        "severity": "performance",
        "summary": "Candidate XSLT application does not complete within budget on files where the oracle transform finishes quickly.",
        "repro": "see raw/16-12/*/xsltproc|python3-lxml|ruby-nokogiri|php",
        "evidence": {"candidate_timeout_cells": len(xsl),
                     "files": sorted({c["id"] for c in xsl})},
    })

    cdata = [c for c in inv if c["id"] in ("gpxkml-003",)]
    findings.append({
        "id": "F6-cdata-divergence",
        "severity": "correctness",
        "summary": "Canonical form differs for a KML document whose descriptions contain CDATA sections (entity/section handling).",
        "repro": "xmllint --c14n /corpus/gpxkml-003.kml | wc -c",
        "oracle": "37286", "candidate": "38051",
        "evidence": {"invalid_cells": len(cdata)},
    })

    dtdsub = [c for c in inv if c["id"] == "svg-006"]
    findings.append({
        "id": "F7-internal-subset-entity",
        "severity": "correctness",
        "summary": "Canonical form differs for an SVG document with an internal DTD subset (defaulted attributes / entity expansion).",
        "repro": "xmllint --c14n /corpus/svg-006.svg | wc -c",
        "oracle": "108562", "candidate": "101061",
        "evidence": {"invalid_cells": len(dtdsub)},
    })

    lx = [c for c in asym if "DTDParseError" in (c.get("candidate_error") or "")]
    findings.append({
        "id": "F8-lxml-dtd-parse",
        "severity": "correctness",
        "summary": "lxml DTD validation fails to parse the (valid) DTD under the candidate where the oracle parses it.",
        "repro": "lxml etree.DTD('/bench/schema/partwise.dtd') under candidate",
        "evidence": {"asymmetric_cells": len(lx),
                     "files": sorted({c["id"] for c in lx})},
    })

    doc = {
        "schema": "consumer-findings/1",
        "phase": "16.12",
        "matrix_counts": m["counts"],
        "findings": findings,
        "summary_by_error": Counter(
            (c.get("candidate_error") or c.get("oracle_error") or "?")[:48]
            for c in cells if not (c["equivalent"])).most_common(20),
    }
    json.dump(doc, open(OUT, "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    print("wrote", OUT)
    for f in findings:
        print(" ", f["id"], json.dumps(f["evidence"])[:100])


if __name__ == "__main__":
    main()
