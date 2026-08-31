#!/usr/bin/env python3
"""CUSTODIAN-COMMENTARY-0001 — forensic custodian commentary court (11.2).

Phase 11.2 requires that compatibility-sensitive code explain, in place:

  1. upstream subsystem/contract          (which upstream module/file/ABI the
                                           code mirrors; the parity target)
  2. conceptual behavior                  (how the subsystem actually works)
  3. ownership/safety invariants          (who owns what; what SAFETY holds)
  4. historical quirks/epochs/commits     (E-00x epochs, QUIRK-*, upstream
                                           commits that created the behavior)
  5. deliberate oddities                  (intentional no-ops, UPSTREAM-PARITY
                                           divergences, odd-but-faithful bits)
  6. proving courts                       (court family / casefile / probe /
                                           receipt that exercises the module)
  7. what a tempting simplification would break
                                           (the "don't simplify this" lessons)

Acceptance (state doc 11.2): a future maintainer can understand *why* the
implementation looks the way it does without reconstructing chat archaeology.

This court mechanically checks every compatibility-sensitive module for the
presence of all seven dimensions in its comments (module header `//!` and
inline `//` / `///` / `//!` blocks). Each dimension has a set of cue patterns;
a module *passes* a dimension when the cue appears in its comment text.

Generated/exempt files (unicode tables, generated mirrors, tiny re-export
stubs) are classified as NOT-APPLICABLE with a documented reason; they still
carry a header stating they are generated. The court emits an immutable
receipt under courts/receipts/phase-11/ and updates nothing else.

Usage:
    python3 tools/evidence/custodian_commentary_court.py
"""
import datetime
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "src")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")

# ── The seven required commentary dimensions (11.2) ────────────────────────
DIMENSIONS = [
    ("upstream_contract",
     "1. upstream subsystem/contract",
     [r"upstream", r"SRC-LIBXML2", r"SRC-LIBXSLT", r"parity target", r"oracle"]),
    ("conceptual_behavior",
     "2. conceptual behavior",
     [r"implements", r"handles", r"behavior", r"semantics", r"model",
      r"architecture", r"state machine", r"engine"]),
    ("ownership_safety",
     "3. ownership/safety invariants",
     [r"SAFETY", r"safety", r"ownership", r"owner", r"invariant",
      r"borrow", r"freed with", r"caller frees"]),
    ("historical_epochs",
     "4. historical quirks/epochs/commits",
     [r"epoch", r"E-00\d", r"quirk", r"QUIRK", r"commit", r"2\.\d+\.\d+",
      r"v2\.", r"since", r"histor"]),
    ("deliberate_oddities",
     "5. deliberate oddities",
     [r"deliberate", r"odd", r"UPSTREAM-PARITY", r"intentional",
      r"no-op", r"quirk", r"divergence"]),
    ("proving_courts",
     "6. proving courts",
     [r"court", r"regress", r"byte-identical", r"receipt", r"probe",
      r"\bPASS\b", r"CLI-[A-Z]+-\d{4}", r"cargo test"]),
    ("tempting_simplification",
     "7. what a tempting simplification would break",
     [r"simplif", r"tempt", r"would break", r"must not", r"don't",
      r"do not", r"naive", r"hazard", r"never", r"not just", r"lesson"]),
]

# Modules that are generated output or trivial re-export stubs: they get a
# documented NOT-APPLICABLE classification rather than a dimension failure.
EXEMPT = {
    "src/abi/exports_xslt_internals.rs": "generated/assembled export registry",
    "src/xml/unicode_tables.rs": "generated table (tools/archaeology/gen_chvalid_tables.py)",
    "src/abi/ucs_blocks.rs": "generated table (tools/abi/gen_ucs_blocks.py)",
    "src/abi/ucs_cat.rs": "generated table (tools/abi/gen_ucs_blocks.py)",
    "src/compatibility/historical/mod.rs": "re-export facade; docs live in atlas/HISTORY.md + SEMANTIC_EPOCHS.md",
    "src/compatibility/platform/mod.rs": "re-export facade; docs live in atlas/PLATFORM_SURFACE_ATLAS.md",
    "src/compatibility/quirks/mod.rs": "re-export facade; docs live in atlas/QUIRKS.md",
}

# Trivial re-export-only facades that must still name their source-of-truth.
FACADE_MIN = {
    "src/xml/parser/mod.rs": r"parser",
    "src/xml/sax/mod.rs": r"sax",
    "src/xml/namespaces/mod.rs": r"namespace",
    "src/xml/xpath/mod.rs": r"xpath",
    "src/xml/mod.rs": r"xml",
    "src/xslt/mod.rs": r"xslt",
    "src/exslt/mod.rs": r"exslt",
    "src/abi/mod.rs": r"abi",
    "src/internal/mod.rs": r"internal",
    "src/internal/globals.rs": r"global",
}


def comment_text(path):
    """Extract all Rust comments (//! /// // and /* */) from a source file.

    The raw comment text is used as-is: cue words legitimately appear inside
    comments, and stripping string literals here would corrupt the analysis
    (a quote inside one comment would pair with a quote far away and swallow
    the cues in between). Code string literals are never scanned, so there is
    no false-positive risk from them.
    """
    with open(path, encoding="utf-8") as f:
        text = f.read()
    blocks = re.findall(r"//[^\n]*|/\*.*?\*/", text, re.S)
    return "\n".join(blocks)


def check_module(rel):
    path = os.path.join(ROOT, rel)
    comments = comment_text(path)
    results = {}
    for key, label, cues in DIMENSIONS:
        hits = [c for c in cues if re.search(c, comments, re.I | re.S)]
        results[key] = {
            "label": label,
            "pass": bool(hits),
            "cues_hit": hits,
        }
    passed = sum(1 for r in results.values() if r["pass"])
    return results, passed


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    modules = []
    for root, _dirs, files in os.walk(SRC):
        for fn in files:
            if fn.endswith(".rs"):
                rel = os.path.relpath(os.path.join(root, fn), ROOT)
                modules.append(rel)
    modules.sort()

    rows = []
    na = []
    fail_rows = []
    for rel in modules:
        if rel in EXEMPT:
            na.append({"module": rel, "classification": "NOT-APPLICABLE",
                       "reason": EXEMPT[rel]})
            continue
        results, passed = check_module(rel)
        if rel in FACADE_MIN:
            # re-export facades must at least name their subsystem in the header
            header = comment_text(os.path.join(ROOT, rel))
            needle = FACADE_MIN[rel]
            if not re.search(needle, header, re.I):
                results["conceptual_behavior"]["pass"] = False
                results["conceptual_behavior"]["cues_hit"] = []
                passed = min(passed, 6)
        row = {"module": rel, "dimensions_passed": passed,
               "dimensions_total": len(DIMENSIONS), "dimensions": results}
        rows.append(row)
        if passed < len(DIMENSIONS):
            missing = [k for k, r in results.items() if not r["pass"]]
            fail_rows.append((rel, missing, results))

    total = len(rows)
    passed_modules = sum(1 for r in rows if r["dimensions_passed"] == len(DIMENSIONS))
    verdict = "PASS" if passed_modules == total and total > 0 else "FAIL"

    receipt = {
        "schema": "custodian-commentary-court-1",
        "court": "CUSTODIAN-COMMENTARY-0001",
        "phase": "11.2",
        "timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "title": "Forensic custodian commentary court (11.2)",
        "dimensions": [{"key": k, "label": l} for k, l, _ in DIMENSIONS],
        "modules_total": total,
        "modules_passed": passed_modules,
        "modules_not_applicable": len(na),
        "not_applicable": na,
        "failures": [
            {"module": rel, "missing_dimensions": missing,
             "missing_cues": {k: r[k] for k in missing}}
            for rel, missing, r in fail_rows
        ],
        "verdict": verdict,
    }
    ts = receipt["timestamp"].replace(":", "").replace("-", "").replace("Z", "Z")
    rpath = os.path.join(RECEIPTS, f"custodian-commentary-{ts}.json")
    with open(rpath, "w") as f:
        json.dump(receipt, f, indent=1)
    print(f"receipt -> {rpath}")
    print(f"modules: {passed_modules}/{total} fully documented, "
          f"{len(na)} NOT-APPLICABLE, verdict={verdict}")
    for rel, missing, _ in fail_rows:
        print(f"  MISSING {rel}: {missing}")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
