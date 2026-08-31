#!/usr/bin/env python3
"""CUSTODIAN-COMMENTARY-DRIFT — reference-drift + coverage-census court
(11.1-Z.2, companion to CUSTODIAN-COMMENTARY-0001).

The 11.2 commentary court proves each module carries the seven custodian
dimensions; this court proves the references inside that commentary stay
RESOLVABLE and that mutable evidence-owned numbers are not embedded in
source. Checks, per src/ module:

  A. no mutable embedded counts — `(N/N)` court-verdict counts (the
     `DSO-LOADER (25/25)` / `HEADER-COMPILE (595/595)` class). The receipts
     own the numbers; source commentary names the court only.
  B. residual references — every `R-\\d{6}` resolves in
     atlas/RESIDUAL_LEDGER.json.
  C. court/probe/casefile references — every `[A-Z][A-Z0-9]+-\\d{3,4}`
     token is a known court family, probe, CLI casefile, or a documented
     non-court ID family (SEC-*/LORE-*/QUIRK-*/ISO-*).
  D. receipt paths — every `courts/receipts/...` mention exists on disk
     (directory or file; globs allowed).
  E. epoch references — every `E-\\d{3}` resolves in SEMANTIC_EPOCHS.md or
     HISTORICAL_SURFACE_EPOCHS.json.

Plus a granular coverage census per module:

  F. every `#[no_mangle]` export has a `///` doc comment immediately above;
  G. every `unsafe fn` (and `unsafe extern "C" fn`) is documented;
  H. every `unsafe {` block has a SAFETY comment in its vicinity.

The drift checks (A-E) gate the verdict; the census (F-H) is recorded with
a documented baseline so coverage trends are visible.

Usage:
    python3 tools/evidence/custodian_commentary_drift.py
"""
import datetime
import glob
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "src")
RECEIPTS = os.path.join(ROOT, "courts", "receipts")
LEDGER = os.path.join(ROOT, "atlas", "RESIDUAL_LEDGER.json")
SEM_EPOCHS = os.path.join(ROOT, "atlas", "SEMANTIC_EPOCHS.md")
HIST_EPOCHS = os.path.join(ROOT, "atlas", "HISTORICAL_SURFACE_EPOCHS.json")
QUIRKS = os.path.join(ROOT, "atlas", "QUIRKS.md")

# ── known court families / probes / casefiles ────────────────────────────────
def build_court_census():
    census = set()
    # receipts: every "court" field recorded in a receipt
    for rp in glob.glob(os.path.join(RECEIPTS, "**", "*.json"), recursive=True):
        try:
            d = json.load(open(rp))
        except Exception:
            continue
        if isinstance(d, dict) and d.get("court"):
            census.add(str(d["court"]))
    # suites: every directory + probe/runner stem under courts/suites
    suites = os.path.join(ROOT, "courts", "suites")
    for root, dirs, files in os.walk(suites):
        for d_ in dirs:
            census.add(d_.upper())
        for f in files:
            stem = f.rsplit(".", 1)[0]
            census.add(stem.upper())
    # CLI casefiles
    for f in glob.glob(os.path.join(ROOT, "courts", "suites", "cli", "*")):
        census.add(os.path.basename(f).upper())
    # the comment-court families named in tooling docstrings
    for f in glob.glob(os.path.join(ROOT, "tools", "abi", "*.py")):
        text = open(f, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"\b[A-Z][A-Z0-9]+-\d{3,4}\b", text):
            census.add(m.group(0))
    for f in glob.glob(os.path.join(ROOT, "tools", "evidence", "*.py")):
        text = open(f, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"\b[A-Z][A-Z0-9]+-\d{3,4}\b", text):
            census.add(m.group(0))
    return census


COURT_CENSUS = build_court_census()

# Non-court ID families that legitimately appear in commentary; each is
# either self-describing or resolves elsewhere.
NON_COURT_PREFIXES = ("SEC-", "LORE-", "QUIRK-", "ISO-", "CVE-", "XPATH-",
                      "SAX-", "DTD-", "XSLT-", "C14N-", "HTML-", "SD-",
                      "IEEE-")
# CLI casefiles are courts of the CLI family (any CLI-<NAME>-NNNN).
CLI_RE = re.compile(r"CLI-[A-Z]+-\d{3,4}$")


def resolve_court_id(tok):
    if tok in COURT_CENSUS:
        return True, "court"
    if CLI_RE.match(tok):
        return True, "cli-casefile"
    if tok.startswith("QUIRK-"):
        q = open(QUIRKS, encoding="utf-8", errors="replace").read()
        return tok in q, "quirk"
    for p in NON_COURT_PREFIXES:
        if tok.startswith(p) and p != "QUIRK-":
            return True, "non-court-family"
    return False, None


def comment_text(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        text = f.read()
    blocks = re.findall(r"//[^\n]*|/\*.*?\*/", text, re.S)
    return "\n".join(blocks)


def epoch_ids():
    ids = set()
    for ep in (SEM_EPOCHS, HIST_EPOCHS):
        if os.path.exists(ep):
            t = open(ep, encoding="utf-8", errors="replace").read()
            ids.update(re.findall(r"E-\d{3}", t))
    return ids


EPOCH_IDS = epoch_ids()


def residual_ids():
    d = json.load(open(LEDGER))
    return {r["id"] for r in d["ledger"]}


RESIDUAL_IDS = residual_ids()


def check_references(rel, comments):
    """Return list of drift findings for one module's comment text."""
    findings = []
    # A. mutable counts
    for m in re.finditer(r"\((\d+/\d+)\)", comments):
        findings.append({"check": "A-mutable-count", "token": m.group(1),
                         "context": _ctx(comments, m.start())})
    # B. residual ids
    for m in re.finditer(r"R-\d{6}", comments):
        if m.group(0) not in RESIDUAL_IDS:
            findings.append({"check": "B-residual-unresolved", "token": m.group(0),
                             "context": _ctx(comments, m.start())})
    # C. court ids (exclude tokens embedded in CLI-<X>-NNNN / CVE-YYYY)
    for m in re.finditer(r"(?<![A-Z0-9-])[A-Z][A-Z0-9]+-\d{3,4}\b", comments):
        tok = m.group(0)
        ok, kind = resolve_court_id(tok)
        if not ok:
            findings.append({"check": "C-court-unresolved", "token": tok,
                             "context": _ctx(comments, m.start())})
    # D. receipt paths
    for m in re.finditer(r"courts/receipts/[A-Za-z0-9_./*-]+", comments):
        p = m.group(0).rstrip(".,;:)")
        hits = glob.glob(os.path.join(ROOT, p))
        if not hits:
            findings.append({"check": "D-receipt-path-unresolved", "token": p,
                             "context": _ctx(comments, m.start())})
    # E. epoch ids (exclude E-YYYY inside CVE-YYYY / IEEE-754 / LORE-NNNN)
    for m in re.finditer(r"(?<![A-Z-])E-\d{3}\b", comments):
        if m.group(0) not in EPOCH_IDS:
            findings.append({"check": "E-epoch-unresolved", "token": m.group(0),
                             "context": _ctx(comments, m.start())})
    return findings


def _ctx(text, pos, width=70):
    return text[max(0, pos - 35):pos + width].replace("\n", " ")


# ── coverage census ──────────────────────────────────────────────────────────
def coverage_census(rel):
    path = os.path.join(ROOT, rel)
    text = open(path, encoding="utf-8", errors="replace").read()
    census = {"exports": {"total": 0, "documented": 0, "undocumented": []},
              "unsafe_fns": {"total": 0, "documented": 0, "undocumented": []},
              "unsafe_blocks": {"total": 0, "safety_commented": 0}}

    # F. #[no_mangle] exports
    for m in re.finditer(
            r"#\[no_mangle\]\s*(?:pub(?:\([^)]*\))?\s*)?"
            r"(?:unsafe\s+)?(?:extern\s+\"C\"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
            text):
        census["exports"]["total"] += 1
        name = m.group(1)
        head = text[:m.start()]
        documented = False
        # strip intervening attribute lines (cfg, allow, ...) between the
        # doc comment block and the #[no_mangle] attribute
        stripped = re.sub(r"#\[[^\]]*\]\s*\n", "", head)
        if re.search(r"///[^\n]*\n(\s*///[^\n]*\n)*\s*$", stripped):
            documented = True
        if documented:
            census["exports"]["documented"] += 1
        else:
            census["exports"]["undocumented"].append(name)

    # G. unsafe fns
    for m in re.finditer(
            r"(?:pub(?:\([^)]*\))?(?:\s+const)?\s+)?unsafe(?:\s+extern\s+\"C\"\s+)?fn\s+"
            r"([A-Za-z_][A-Za-z0-9_]*)",
            text):
        census["unsafe_fns"]["total"] += 1
        name = m.group(1)
        head = text[:m.start()]
        stripped = re.sub(r"#\[[^\]]*\]\s*\n", "", head)
        # drop a trailing `pub [const] ` prefix left by the fn-arity match
        stripped = re.sub(r"(?:pub\s*)?(?:const\s*)?$", "", stripped.rstrip())
        if re.search(r"///[^\n]*\n(\s*///[^\n]*\n)*\s*$", stripped + "\n"):
            census["unsafe_fns"]["documented"] += 1
        else:
            census["unsafe_fns"]["undocumented"].append(name)

    # H. unsafe blocks with a SAFETY comment in the vicinity
    for m in re.finditer(r"unsafe\s*\{", text):
        census["unsafe_blocks"]["total"] += 1
        window = text[max(0, m.start() - 400):m.end() + 400]
        if re.search(r"//\s*SAFETY\b|#\s*Safety\b", window, re.I):
            census["unsafe_blocks"]["safety_commented"] += 1
    return census


def main():
    os.makedirs(os.path.join(RECEIPTS, "phase-11"), exist_ok=True)
    modules = []
    for root, _dirs, files in os.walk(SRC):
        for fn in files:
            if fn.endswith(".rs"):
                rel = os.path.relpath(os.path.join(root, fn), ROOT)
                modules.append(rel)
    modules.sort()

    all_findings = []
    census_rows = []
    for rel in modules:
        comments = comment_text(os.path.join(ROOT, rel))
        for f_ in check_references(rel, comments):
            f_["module"] = rel
            all_findings.append(f_)
        census_rows.append({"module": rel, **coverage_census(rel)})

    # aggregate census
    agg = {"exports": {"total": 0, "documented": 0},
           "unsafe_fns": {"total": 0, "documented": 0},
           "unsafe_blocks": {"total": 0, "safety_commented": 0}}
    for r in census_rows:
        for k in ("exports", "unsafe_fns", "unsafe_blocks"):
            agg[k]["total"] += r[k]["total"]
            if k == "unsafe_blocks":
                agg[k]["safety_commented"] += r[k]["safety_commented"]
            else:
                agg[k]["documented"] += r[k]["documented"]
    for k in ("exports", "unsafe_fns"):
        agg[k]["undocumented"] = [x for r in census_rows
                                  for x in r[k]["undocumented"]]
    agg["unsafe_blocks"]["unsafety_commented"] = (
        agg["unsafe_blocks"]["total"] - agg["unsafe_blocks"]["safety_commented"])

    verdict = "PASS" if not all_findings else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "CUSTODIAN-COMMENTARY-DRIFT",
        "phase": "11.1-Z.2",
        "timestamp": ts,
        "schema": "custodian-commentary-drift-1",
        "checks": {
            "A-mutable-count": "no (N/N) court-verdict counts embedded in source",
            "B-residual-unresolved": "every R-\\d{6} resolves in the residual ledger",
            "C-court-unresolved": "every court/probe/casefile ID is known",
            "D-receipt-path-unresolved": "every courts/receipts mention exists",
            "E-epoch-unresolved": "every E-\\d{3} resolves in the epochs atlas",
        },
        "modules_total": len(modules),
        "findings": all_findings,
        "summary": {k: sum(1 for f_ in all_findings if f_["check"] == k)
                    for k in ("A-mutable-count", "B-residual-unresolved",
                              "C-court-unresolved", "D-receipt-path-unresolved",
                              "E-epoch-unresolved")},
        "coverage_census": agg,
        "coverage_per_module": census_rows,
        "verdict": verdict,
    }
    rp = os.path.join(RECEIPTS, "phase-11", f"custodian-commentary-drift-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"modules={len(modules)} findings={len(all_findings)} verdict={verdict}")
    for k, v in receipt["summary"].items():
        print(f"  {k}: {v}")
    print("coverage census: "
          f"exports {agg['exports']['documented']}/{agg['exports']['total']} documented, "
          f"unsafe-fns {agg['unsafe_fns']['documented']}/{agg['unsafe_fns']['total']} documented, "
          f"unsafe-blocks {agg['unsafe_blocks']['safety_commented']}/{agg['unsafe_blocks']['total']} SAFETY-commented")
    for f_ in all_findings[:40]:
        print(f"  [{f_['check']}] {f_['module']}: {f_.get('token')} — {f_.get('context', '')[:60]}")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
