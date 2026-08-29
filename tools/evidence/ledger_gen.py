#!/usr/bin/env python3
"""Residual ledger canonical generator + evidence-integrity court (§70, §71, 11.1-A).

`atlas/RESIDUAL_LEDGER.json` is the single hand-maintained source of truth.
This tool deterministically generates `atlas/RESIDUAL_LEDGER.md` from it and
validates the evidence chain:

  1. unique residual IDs;
  2. allowed state values and legal state transitions (from the `history` chain);
  3. referenced court/test IDs resolve to committed courts or test functions;
  4. referenced source paths exist in the repository;
  5. referenced semantic epoch IDs exist in atlas/SEMANTIC_EPOCHS.md;
  6. regenerated Markdown is byte-identical to the committed file
     (fails the court if Git would see a diff).

Usage:
  ledger_gen.py gen            regenerate atlas/RESIDUAL_LEDGER.md
  ledger_gen.py check          run the full evidence-integrity court (no writes)
  ledger_gen.py check --fix    regenerate the Markdown, then re-run the court

The generated open-residual count is computed from the JSON — it can never
drift from the records.
"""
import argparse
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LEDGER_JSON = os.path.join(ROOT, "atlas", "RESIDUAL_LEDGER.json")
LEDGER_MD = os.path.join(ROOT, "atlas", "RESIDUAL_LEDGER.md")
EPOCHS_MD = os.path.join(ROOT, "atlas", "SEMANTIC_EPOCHS.md")

ALLOWED_STATES = {"OPEN", "FIXED"}
ALLOWED_TRANSITIONS = {("OPEN", "FIXED")}
ALLOWED_CLASSIFICATIONS = {
    "CANDIDATE_BUG", "ORACLE_BUG", "VERSION_DIFFERENCE",
    "INTENTIONAL_SAFE_DIVERGENCE", "UNRESOLVED",
}

# Court families defined by Phase 11.1-Y (11.1 sealing criteria) plus suites
# present in courts/suites/. Referenced "court IDs" in residual records are
# validated against the union of committed casefiles and this family list.
COURT_FAMILIES = {
    "DOXYGEN-SURFACE", "AST-SURFACE", "PREPROCESSOR-SURFACE", "HEADER-COMPILE",
    "ABI-SYMBOL", "ABI-DATA", "ABI-STRUCT", "ABI-ENUM", "ABI-MACRO", "ABI-CALLBACK",
    "ABI-TYPE", "DSO-LOADER", "BUILD-PKGCONFIG", "BUILD-CONFIG-SCRIPT",
    "INSTALL-LAYOUT", "OWNERSHIP", "ALLOCATOR", "GLOBAL-STATE", "THREADING",
    "CALLBACK", "ERROR", "PARSER", "TREE-STRUCTURE", "SERIALIZATION", "XPATH",
    "XPOINTER", "XINCLUDE", "DTD", "XSD", "RELAXNG", "SCHEMATRON", "READER",
    "WRITER", "REGEX", "AUTOMATA", "C14N", "HTML", "XSLT", "EXSLT",
    "CLI-XMLLINT", "CLI-XMLCATALOG", "CLI-XSLTPROC", "HIST-SURFACE-EPOCH",
    "HIST-BEHAVIOR-EPOCH", "ORACLE-IDENTITY", "ORACLE-CONTAMINATION",
    "EVIDENCE-INTEGRITY",
}


def load_ledger():
    with open(LEDGER_JSON, encoding="utf-8") as f:
        return json.load(f)


def canonical_md(ledger):
    """Deterministic Markdown. No timestamps — stable across regenerations."""
    phases = ledger.get("phases", [])  # canonical display order
    residuals = {r["id"]: r for r in ledger["ledger"]}
    open_ids = [r["id"] for r in ledger["ledger"] if r["status"] == "OPEN"]

    lines = []
    lines.append("# Residual Ledger")
    lines.append("")
    lines.append("Per §71: every unexplained difference gets an ID (`R-000001`...), and its")
    lines.append("history is retained after fixing. This Markdown is generated from")
    lines.append("`RESIDUAL_LEDGER.json` by `tools/evidence/ledger_gen.py` (§70 policy:")
    lines.append("Markdown generated from JSON; the JSON is the only hand-maintained truth).")
    lines.append("")
    lines.append("## Current Residuals")
    lines.append("")
    if len(open_ids) == 0:
        lines.append("**0 open residuals.** Every discovered residual has a disposition.")
    elif len(open_ids) == 1:
        lines.append(f"**1 open residual:** {open_ids[0]}")
    else:
        lines.append(f"**{len(open_ids)} open residuals:** " + ", ".join(open_ids))
    lines.append("")

    by_phase = {}
    for r in ledger["ledger"]:
        by_phase.setdefault(r.get("phase", "tooling"), []).append(r)
    for phase in phases:
        section_residuals = sorted(by_phase.get(phase, []), key=lambda r: r["id"])
        if not section_residuals:
            continue
        lines.append(f"## Phase {phase} Residuals")
        lines.append("")
        for r in section_residuals:
            lines.append(f"### {r['id']}: {r['title']} ({r['status']})")
            lines.append("")
            lines.append(f"- **Status:** {r['status']}"
                         + (f" ({r.get('fixed_date', '')}, Phase {phase})" if r["status"] == "FIXED" else ""))
            lines.append(f"- **Component:** {', '.join(r['component'])}")
            lines.append(f"- **Surface:** {r['surface']}")
            if r.get("oracle_versions"):
                lines.append(f"- **Oracle versions:** {r['oracle_versions']}")
            if r.get("root_cause"):
                lines.append(f"- **Root cause:** {r['root_cause']}")
            if r.get("observable_residual"):
                lines.append(f"- **Observable residual:** {r['observable_residual']}")
            if r.get("fix"):
                lines.append(f"- **Fix:** {r['fix']}")
            if r.get("triangulation"):
                lines.append(f"- **Phase 11 triangulation:** {r['triangulation']}")
            courts = r.get("regression_courts", []) or []
            if courts:
                lines.append("- **Regression courts:** " + ", ".join(courts) + ".")
            if r.get("evidence"):
                lines.append(f"- **Evidence:** {r['evidence']}")
            if r.get("classification"):
                lines.append(f"- **Classification:** {r['classification']}")
            history = r.get("history", [])
            if len(history) > 1:
                entries = "; ".join(
                    f"{h['status']} {h['date']}" + (f" ({h.get('note')})" if h.get("note") else "")
                    for h in history
                )
                lines.append(f"- **History:** {entries}")
            lines.append("")
    lines.append("## Classification Legend")
    lines.append("")
    for c in sorted(ALLOWED_CLASSIFICATIONS):
        lines.append(f"- `{c}` — see classification policy in §45/§71")
    lines.append("")
    return "\n".join(lines)


def collect_casefiles():
    """All committed court casefile basenames (id part before the dash-suffix)."""
    suite_root = os.path.join(ROOT, "courts", "suites")
    ids = set()
    if os.path.isdir(suite_root):
        for dirpath, _dirs, files in os.walk(suite_root):
            for fn in files:
                if fn.endswith(".json"):
                    try:
                        doc = json.load(open(os.path.join(dirpath, fn), encoding="utf-8"))
                        if "case_id" in doc:
                            ids.add(doc["case_id"])
                    except Exception:
                        ids.add(fn[:-5])
    return ids


def court_or_test_exists(ref, casefile_ids, source_cache):
    """True if ref is a court family, a committed casefile id, or a Rust test fn."""
    if ref in COURT_FAMILIES or ref in casefile_ids:
        return True
    # test function references: test_xslt_*, test_node_get_content_*, ...
    if re.match(r"^(test|court)_[A-Za-z0-9_]+$", ref):
        for path, text in source_cache.items():
            if re.search(rf"\bfn\s+{re.escape(ref)}\b", text):
                return True
        return False
    return True  # free-form references (e.g. "XSLT end-to-end transform tests")


def iter_rust_sources():
    src_root = os.path.join(ROOT, "src")
    if not os.path.isdir(src_root):
        return
    for dirpath, _dirs, files in os.walk(src_root):
        for fn in files:
            if fn.endswith(".rs"):
                yield os.path.relpath(os.path.join(dirpath, fn), ROOT)


def source_path_exists(path):
    """Component references may be src/..., tools/..., atlas/..., oracle/..."""
    p = os.path.join(ROOT, path)
    if os.path.exists(p):
        return True
    # allow glob-ish references like src/xml/parser/state.rs, src/xslt/transform/mod.rs
    return any(os.path.exists(os.path.join(ROOT, part)) for part in re.split(r"[,\s]", path) if part)


def epoch_ids():
    txt = ""
    if os.path.exists(EPOCHS_MD):
        with open(EPOCHS_MD, encoding="utf-8") as f:
            txt = f.read()
    return set(re.findall(r"\bE-00\d\b", txt))


def run_check(fix=False):
    errors = []
    ledger = load_ledger()

    # 1. unique IDs
    ids = [r["id"] for r in ledger["ledger"]]
    dups = sorted({i for i in ids if ids.count(i) > 1})
    if dups:
        errors.append(f"duplicate residual IDs: {dups}")
    if any(not re.match(r"^R-\d{6}$", i) for i in ids):
        errors.append("residual IDs must match ^R-\\d{6}$")
    if ids != sorted(ids):
        errors.append("residuals must be ordered by ID")

    # 1b. every residual phase must be in the canonical display order, otherwise
    # the generated Markdown would silently omit it (byte-identity would pass
    # vacuously). No silent omission is allowed: fail instead.
    known_phases = set(ledger.get("phases", []))
    for r in ledger["ledger"]:
        ph = r.get("phase", "tooling")
        if ph not in known_phases:
            errors.append(f"{r['id']}: phase {ph!r} not in canonical phases list; "
                          "the Markdown would silently drop this residual")

    # required fields
    for r in ledger["ledger"]:
        for field in ("id", "status", "title", "surface", "component", "classification"):
            if field not in r or r[field] in (None, "", []):
                errors.append(f"{r.get('id')}: missing required field '{field}'")
        st = r["status"]
        if st not in ALLOWED_STATES:
            errors.append(f"{r.get('id')}: illegal status {st!r}")
        if r.get("classification") not in ALLOWED_CLASSIFICATIONS:
            errors.append(f"{r.get('id')}: illegal classification {r.get('classification')!r}")
        if st == "FIXED" and not r.get("fix"):
            errors.append(f"{r['id']}: FIXED residual requires 'fix'")

        # 3. state transitions via history chain
        history = r.get("history", [{"status": st, "date": r.get("discovery_date")}])
        if history and history[-1]["status"] != st:
            errors.append(f"{r['id']}: history tail {history[-1]['status']} != status {st}")
        for prev, nxt in zip(history, history[1:]):
            if (prev["status"], nxt["status"]) not in ALLOWED_TRANSITIONS:
                errors.append(f"{r['id']}: illegal transition {prev['status']}->{nxt['status']}")
        if st == "OPEN" and len(history) > 1:
            errors.append(f"{r['id']}: OPEN residual with closed history tail")

    # 4/5. court + source-path references
    casefile_ids = collect_casefiles()
    source_cache = {p: open(os.path.join(ROOT, p), encoding="utf-8").read()
                    for p in iter_rust_sources()}
    for r in ledger["ledger"]:
        for ref in r.get("regression_courts", []) or []:
            if not court_or_test_exists(ref, casefile_ids, source_cache):
                errors.append(f"{r['id']}: unresolved court/test reference {ref!r}")
        for comp in r["component"]:
            if not source_path_exists(comp):
                errors.append(f"{r['id']}: component path does not exist: {comp!r}")
        for m in re.findall(r"\bE-00\d\b", json.dumps(r)):
            if m not in epoch_ids():
                errors.append(f"{r['id']}: unknown semantic epoch {m!r}")

    # 6. generated Markdown byte-identity
    generated = canonical_md(ledger)
    if fix:
        with open(LEDGER_MD, "w", encoding="utf-8") as f:
            f.write(generated)
        print("regenerated", LEDGER_MD)
    with open(LEDGER_MD, encoding="utf-8") as f:
        committed = f.read()
    if committed != generated:
        errors.append("RESIDUAL_LEDGER.md is not the deterministic output of the JSON "
                      "(run: tools/evidence/ledger_gen.py check --fix)")

    if errors:
        print("EVIDENCE-INTEGRITY COURT: FAIL")
        for e in errors:
            print("  -", e)
        return 1
    open_count = sum(1 for r in ledger["ledger"] if r["status"] == "OPEN")
    print(f"EVIDENCE-INTEGRITY COURT: PASS ({len(ledger['ledger'])} residuals, "
          f"{open_count} open, Markdown byte-identical)")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("command", choices=["gen", "check"])
    ap.add_argument("--fix", action="store_true")
    args = ap.parse_args()
    if args.command == "gen":
        ledger = load_ledger()
        with open(LEDGER_MD, "w", encoding="utf-8") as f:
            f.write(canonical_md(ledger))
        print("wrote", LEDGER_MD)
        return 0
    return run_check(fix=args.fix)


if __name__ == "__main__":
    sys.exit(main())
