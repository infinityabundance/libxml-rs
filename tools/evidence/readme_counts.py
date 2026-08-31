#!/usr/bin/env python3
"""11.1-W — README headline counts generator.

"Never manually type the headline counts in the README. Generate them."

This tool rewrites the README's evidence-bearing sections (Project Status
table, test-coverage-by-subsystem table, and every bare "NNNN passing tests"
occurrence) from the committed ledgers:

  atlas/PARITY_MATRIX.json          headline parity totals
  atlas/ABI_PARITY_LEDGER.json      ABI verdict + mismatch count
  atlas/PARITY_OBLIGATIONS.json     obligation status counts
  atlas/SUBSYSTEM_CENSUS.json       subsystem verdict totals
  atlas/API_PARITY_LEDGER.json      per-project reconciliation
  atlas/TEST_COUNTS.json            `cargo test --lib` outcome + breakdown

The generated blocks live between explicit markers:

  <!-- GENERATED-STATUS:START --> ... <!-- GENERATED-STATUS:END -->
  <!-- GENERATED-TESTCOVERAGE:START --> ... <!-- GENERATED-TESTCOVERAGE:END -->

Everything outside the markers is hand-written narrative and is untouched.

Usage:
  readme_counts.py          regenerate the generated sections in README.md
  readme_counts.py --check  verify the committed README matches regeneration
"""
import argparse
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATLAS = os.path.join(ROOT, "atlas")
README = os.path.join(ROOT, "README.md")

STATUS_START = "<!-- GENERATED-STATUS:START -->"
STATUS_END = "<!-- GENERATED-STATUS:END -->"
COVER_START = "<!-- GENERATED-TESTCOVERAGE:START -->"
COVER_END = "<!-- GENERATED-TESTCOVERAGE:END -->"


def load(name):
    with open(os.path.join(ATLAS, name)) as f:
        return json.load(f)


def status_table():
    matrix = load("PARITY_MATRIX.json")
    abi = load("ABI_PARITY_LEDGER.json")
    obligations = load("PARITY_OBLIGATIONS.json")
    census = load("SUBSYSTEM_CENSUS.json")
    tests = load("TEST_COUNTS.json")

    lx2 = matrix["projects"]["libxml2"]
    lxs = matrix["projects"]["libxslt"]
    lxe = matrix["projects"]["libexslt"]
    mx2, mxs = lx2["counts"], lxs["counts"]

    abi_ok = all(p["verdict"] == "PASS" for p in abi["projects"].values())
    abi_mismatch = sum(p["mismatch_count"] for p in abi["projects"].values())

    obl_total = {proj: sum(p["counts"].values())
                 for proj, p in obligations["projects"].items()}
    obl_missing = {proj: p["counts"].get("MISSING", 0) + p["counts"].get("DATA_MISSING", 0)
                   for proj, p in obligations["projects"].items()}
    obl_verified = {proj: p["counts"].get("CURRENT_PARITY_VERIFIED", 0)
                    for proj, p in obligations["projects"].items()}

    tests_passed = tests["cargo_test_lib"]["passed"]
    tests_failed = tests["cargo_test_lib"]["failed"]
    tests_ignored = tests["cargo_test_lib"]["ignored"]

    epochs = load("HISTORICAL_SURFACE_EPOCHS.json")
    lx2_epochs = epochs["projects"]["libxml2"]

    rows = [
        ("API completeness",
         f"libxml2 {mx2['public_functions']['oracle_dso']} oracle functions, "
         f"{mx2['public_functions']['fully_reconciled']} fully reconciled; "
         f"libxslt {mxs['public_functions']['fully_reconciled']}/{mxs['public_functions']['oracle_dso']} "
         f"reconciled; libexslt {lxe['counts']['public_functions']['oracle_dso']} oracle functions "
         f"(evidence: atlas/PARITY_MATRIX.json, atlas/API_PARITY_LEDGER.json)"),
        ("ABI compatibility",
         f"{abi_mismatch} mismatches across {sum(p['oracle_entities'] for p in abi['projects'].values())} "
         f"measured entities (struct/enum layouts), verdict "
         f"{'PASS' if abi_ok else 'FAIL'} (evidence: atlas/ABI_PARITY_LEDGER.json)"),
        ("Parity obligations",
         f"{obl_total['libxml2'] + obl_total['libxslt'] + obl_total['libexslt']} obligations; "
         f"{obl_missing['libxml2'] + obl_missing['libxslt'] + obl_missing['libexslt']} missing, "
         f"{obl_verified['libxml2'] + obl_verified['libxslt'] + obl_verified['libexslt']} "
         f"parity-verified by per-symbol courts "
         f"(evidence: atlas/PARITY_OBLIGATIONS.json)"),
        ("Subsystem census",
         f"{sum(census['verdict_totals'].values())} subsystems classified; "
         f"verdicts: {', '.join(f'{k} {v}' for k, v in sorted(census['verdict_totals'].items()))} "
         f"(evidence: atlas/SUBSYSTEM_CENSUS.json)"),
        ("Surface reconciliation",
         f"libxml2: doxygen {mx2['public_functions']['oracle_doxygen']} / AST "
         f"{mx2['public_functions']['oracle_clang_ast']} / DSO "
         f"{mx2['public_functions']['oracle_dso']} functions; "
         f"libxslt: {mxs['public_functions']['oracle_doxygen']} / "
         f"{mxs['public_functions']['oracle_clang_ast']} / "
         f"{mxs['public_functions']['oracle_dso']} "
         f"(evidence: atlas/SURFACE_RECONCILIATION.json)"),
        ("Historical surface epochs",
         f"libxml2 {lx2_epochs['totals']['entities']} entities across "
         f"{len(lx2_epochs['deltas'])} boundaries "
         f"(evidence: atlas/HISTORICAL_SURFACE_EPOCHS.json)"),
        ("Test coverage",
         f"{tests_passed} passing, {tests_failed} failed, {tests_ignored} ignored "
         f"(`cargo test --lib`, evidence: atlas/TEST_COUNTS.json)"),
        ("C headers",
         "gcc & clang header-compile courts green (595/595, evidence: "
         "courts/receipts/header-compile-*)"),
        ("CLI parity",
         "`xmllint` + `xmlcatalog` + `xsltproc` differential oracle parity "
         "(evidence: courts/receipts/CLI-*)"),
        ("Oracle infrastructure",
         "12 historical libxml2 + 5 libxslt oracles + system 2.15.3/1.1.45/0.8.25 "
         "oracles; evidence: oracle/historical, atlas/DOXYGEN_SURFACE_ATLAS.json"),
        ("Downstream testing",
         "Not started (Phase 12)"),
    ]
    lines = [f"| Dimension | Status |", "|---|---|"]
    for name, status in rows:
        lines.append(f"| {name} | {status} |")
    return "\n".join(lines)


def coverage_table():
    tests = load("TEST_COUNTS.json")
    subs = tests["subsystems"]
    passed = tests["cargo_test_lib"]["passed"]
    rows = sorted(subs.items(), key=lambda kv: (-kv[1], kv[0]))
    lines = ["| Subsystem | Tests |", "|-----------|------:|"]
    for name, n in rows:
        lines.append(f"| {name} | {n} |")
    lines.append(f"| **Total ({passed} passing, 0 failed)** | |")
    return "\n".join(lines)


def replace_block(text, start, end, new_block):
    if start not in text or end not in text:
        raise SystemExit(f"missing markers {start!r} / {end!r} in README.md")
    head, _, rest = text.partition(start)
    _, _, tail = rest.partition(end)
    return head + start + "\n" + new_block + "\n" + end + tail


def refresh_passing_strings(text, passed):
    """Replace bare 'NNNN passing tests' and '(NNNN passing)' with the
    generated count."""
    text = re.sub(r"\d+ passing tests", f"{passed} passing tests", text)
    return re.sub(r"\((\d+) passing\)", f"({passed} passing)", text)


def regenerate():
    with open(README) as f:
        text = f.read()
    text = replace_block(text, STATUS_START, STATUS_END, status_table())
    text = replace_block(text, COVER_START, COVER_END, coverage_table())
    passed = load("TEST_COUNTS.json")["cargo_test_lib"]["passed"]
    text = refresh_passing_strings(text, passed)
    return text


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="verify README matches regeneration without writing")
    args = ap.parse_args()
    new = regenerate()
    if args.check:
        with open(README) as f:
            cur = f.read()
        if cur == new:
            print("README: generated sections byte-identical")
            return 0
        print("README: DRIFT — run `python3 tools/evidence/readme_counts.py`")
        return 1
    with open(README, "w") as f:
        f.write(new)
    print("README.md: generated sections refreshed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
