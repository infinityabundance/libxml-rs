#!/usr/bin/env python3
"""11.1-I function-level parity obligation ledger generator (schema v2).

For every externally relevant function/data object exported by the ORACLE
DSOs (system libxml2/libxslt), records a machine-readable parity obligation.

Schema v2 makes verification ORTHOGONAL. "Symbol exists" is never treated as
"function is verified". Each obligation carries six independent dimensions:

    export_status        MISSING | EXPORTED
    implementation_status STUB | INTENTIONAL_NOOP | IMPLEMENTED | NOT_APPLICABLE
    abi_status           UNVERIFIED | PASS | FAIL
    semantic_status      UNVERIFIED | PARTIAL | PASS | FAIL
    ownership_status     NOT_APPLICABLE | UNVERIFIED | PASS
    historical_status    CURRENT_ONLY | PARTIAL | VERIFIED

`overall` is DERIVED, never handwritten:

    MISSING               export_status == MISSING
    STUB                  implementation_status == STUB
    INTENTIONAL_NOOP      implementation_status == INTENTIONAL_NOOP
    PARITY_VERIFIED       EXPORTED + IMPLEMENTED + abi PASS
                          + semantic PASS + ownership (NA or PASS)
    IMPLEMENTED_UNVERIFIED otherwise

`PARITY_VERIFIED` therefore requires a passing per-symbol court; today no
per-symbol courts exist yet, so no entry earns it. The per-symbol court
results live in atlas/SYMBOL_COURT_INDEX.json (schema published, populated as
courts land) and are consumed here so the ledger stays current without
hand-editing.

Usage:
  parity_obligations.py          # regenerate atlas/PARITY_OBLIGATIONS.json
"""
import json
import os
import re
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATLAS = os.path.join(ROOT, "atlas")

ORACLE_DSOS = {
    "libxml2": "/usr/lib/libxml2.so.2",
    "libxslt": "/usr/lib/libxslt.so.1",
}
CANDIDATE_DSO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
PREFIXES = {"libxml2": ("xml", "html", "__xml"), "libxslt": ("xslt", "exslt")}
SYMBOL_COURT_INDEX = os.path.join(ATLAS, "SYMBOL_COURT_INDEX.json")

STATUS_MODEL = {
    "schema": "parity-obligations-status-model-1",
    "export_status": ["MISSING", "EXPORTED"],
    "implementation_status": ["STUB", "INTENTIONAL_NOOP", "IMPLEMENTED", "UNKNOWN", "NOT_APPLICABLE"],
    "abi_status": ["UNVERIFIED", "PASS", "FAIL"],
    "semantic_status": ["UNVERIFIED", "PARTIAL", "PASS", "FAIL"],
    "ownership_status": ["NOT_APPLICABLE", "UNVERIFIED", "PASS"],
    "historical_status": ["CURRENT_ONLY", "PARTIAL", "VERIFIED"],
    "overall": ["MISSING", "STUB", "INTENTIONAL_NOOP",
                "IMPLEMENTED_UNVERIFIED", "PARITY_VERIFIED",
                "DATA_EXPORTED", "DATA_MISSING"],
    "parity_verified_definition": (
        "EXPORTED + IMPLEMENTED + abi_status PASS + semantic_status PASS "
        "+ ownership_status in (NOT_APPLICABLE, PASS) — each supported by a "
        "passing court recorded in courts[]"),
}


def dso_symbols(path, prefixes):
    """Return {symbol: kind} where kind is the nm type letter (T/t=func,
    D/d/B/b=object, R/r=rodata, ...)."""
    r = subprocess.run(["nm", "-D", "--defined-only", path],
                       capture_output=True, text=True)
    out = {}
    for line in r.stdout.splitlines():
        parts = line.split()
        if len(parts) == 3:
            sym = parts[2]
            if "@" in sym:
                sym = sym.split("@")[0]
            if sym.startswith(prefixes):
                out[sym] = parts[1]
    return out


def rust_sources():
    src = os.path.join(ROOT, "src")
    for root, _dirs, files in os.walk(src):
        for fn in files:
            if fn.endswith(".rs"):
                yield os.path.join(root, fn)


def stub_score(name, text):
    """Detect stub/placeholder bodies for a #[no_mangle] function.

    Returns a (implementation_status, reason) pair where implementation_status
    is one of 'IMPLEMENTED' (has real logic), 'STUB' (placeholder detected),
    or 'UNKNOWN' (no source body found / macro export).

    NOTE: this is a *static* heuristic, never a semantic verification. It only
    classifies the implementation dimension; abi/semantic/ownership statuses
    come from courts.
    """
    # find `fn name(` in the text (with optional visibility/extern attrs)
    m = re.search(r"fn\s+" + re.escape(name) + r"\s*\(", text)
    if not m:
        return "UNKNOWN", "no source body found"
    # find the body: next { ... } at top level, bounded
    i = text.find("{", m.end())
    if i == -1:
        return "UNKNOWN", "no body brace"
    depth = 0
    j = i
    n = len(text)
    while j < n:
        c = text[j]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                break
        j += 1
    body = text[i:j + 1]
    if "todo!" in body or "unimplemented!" in body or "unreachable!()" in body:
        return "STUB", "todo!/unimplemented!"
    if re.search(r"panic!\s*\(", body):
        return "STUB", "panic! on API path"
    # unconditional trivial return: return 0 / -1 / NULL / () with no other
    # statements (allow comments)
    stripped = re.sub(r"//[^\n]*\n", "", body)
    stripped = re.sub(r"/\*.*?\*/", "", stripped, flags=re.S)
    stmts = [s.strip() for s in re.split(r"[;{}]", stripped) if s.strip()]
    rets = [s for s in stmts if s.startswith("return")]
    if len(stmts) == 1 and rets and re.match(r"return\s*(0|-1|NULL|null_mut\(\)|ptr::null_mut\(\));?$",
                                             rets[0]):
        return "STUB", f"unconditional trivial return: {rets[0]}"
    # no-op: empty body (only comments/whitespace)
    if not re.search(r"[a-zA-Z_]", re.sub(r"//[^\n]*\n", "", body)):
        return "STUB", "empty body"
    return "IMPLEMENTED", "has logic"


# Documented intentional no-ops / simplified implementations whose bodies are
# intentionally empty or trivial (see the referenced residual for each).
DOCUMENTED_NOOPS = {
    "xmlInitMemory": "R-000131",
    "xmlCleanupMemory": "R-000131",
    "xmlInitGlobals": "R-000131",
    "xmlCleanupGlobals": "R-000131",
    "xmlMemSize": "R-000131",
    "xmlMemShow": "R-000131",
    "xmlAutomataSetFinalState": "R-000135",
    "xmlAutomataNewCounter": "R-000135",
    # deprecated init/cleanup entry points that are genuine no-ops in
    # modern libxml2 (the subsystems initialize lazily); the candidate
    # matches that observable behavior
    "xmlInitializeGlobalState": "R-000138",
    "xmlInitializeDict": "R-000138",
    "xmlInitializePredefinedEntities": "R-000138",
    "xmlCleanupPredefinedEntities": "R-000138",
    "xmlDefaultSAXHandlerInit": "R-000138",
    "xmlCheckThreadLocalStorage": "R-000138",
}


def load_symbol_courts():
    """Per-symbol court verdicts (atlas/SYMBOL_COURT_INDEX.json).

    Schema:
      { "schema": "symbol-court-index-1",
        "courts": { "<court-id>": { "symbols": { "<proj>:<sym>": {
             "abi": "PASS", "semantic": "PASS", "ownership": "PASS" } } } } }
    Populated as per-symbol courts land; the parity ledger consumes it so
    verification flows from courts, never from hand-edited flags.
    """
    if not os.path.exists(SYMBOL_COURT_INDEX):
        return {}
    d = json.load(open(SYMBOL_COURT_INDEX))
    out = {}
    for cid, cdata in (d.get("courts") or {}).items():
        for key, verdicts in (cdata.get("symbols") or {}).items():
            out[key] = verdicts
    return out


def derive_overall(entry):
    if entry["export_status"] == "MISSING":
        return "DATA_MISSING" if entry["kind"] == "DATA" else "MISSING"
    if entry["kind"] == "DATA":
        return "DATA_EXPORTED"
    if entry["implementation_status"] == "STUB":
        return "STUB"
    if entry["implementation_status"] == "INTENTIONAL_NOOP":
        return "INTENTIONAL_NOOP"
    if entry["implementation_status"] != "IMPLEMENTED":
        return "IMPLEMENTED_UNVERIFIED"
    if (entry["abi_status"] == "PASS"
            and entry["semantic_status"] == "PASS"
            and entry["ownership_status"] in ("NOT_APPLICABLE", "PASS")):
        return "PARITY_VERIFIED"
    return "IMPLEMENTED_UNVERIFIED"


def main():
    hay = "\n".join(open(p, encoding="utf-8", errors="replace").read()
                    for p in rust_sources())
    symbol_courts = load_symbol_courts()
    ledger = {"schema": "parity-obligations-2",
              "generated": __import__("datetime").datetime.now(
                  __import__("datetime").timezone.utc)
              .strftime("%Y-%m-%dT%H:%M:%SZ"),
              "status_model": STATUS_MODEL,
              "symbol_court_index": os.path.relpath(SYMBOL_COURT_INDEX, ROOT),
              "verification_policy": (
                  "PARITY_VERIFIED is earned only when a per-symbol court "
                  "exists and passes (abi + semantic + ownership). "
                  "IMPLEMENTED_UNVERIFIED means the export exists with real "
                  "logic but has not yet been verified by a court."),
              "projects": {}}
    for project, path in ORACLE_DSOS.items():
        if not os.path.exists(path):
            print(f"skip {project}: {path} not found")
            continue
        oracle = dso_symbols(path, PREFIXES[project])
        cand = dso_symbols(CANDIDATE_DSO, PREFIXES[project]) if \
            os.path.exists(CANDIDATE_DSO) else {}
        entries = []
        for sym in sorted(oracle):
            kind = oracle[sym]
            data = not (kind in ("T", "t"))
            exported = sym in cand
            impl = "NOT_APPLICABLE" if data else "UNKNOWN"
            reason = None
            if exported and not data:
                impl, reason = stub_score(sym, hay)
                if impl in ("STUB", "UNKNOWN") and sym in DOCUMENTED_NOOPS:
                    impl = "INTENTIONAL_NOOP"
                    reason = f"documented intentional no-op (residual {DOCUMENTED_NOOPS[sym]})"
            elif not exported and sym in DOCUMENTED_NOOPS:
                # not exported but documented as an intentional no-op surface:
                # keep it classified as an unexported obligation (still MISSING)
                reason = f"documented intentional no-op surface (residual {DOCUMENTED_NOOPS[sym]}); not exported"

            courts = []
            residuals = [DOCUMENTED_NOOPS[sym]] if sym in DOCUMENTED_NOOPS else []
            abi = "UNVERIFIED"
            sem = "UNVERIFIED"
            own = "NOT_APPLICABLE" if data else "UNVERIFIED"
            hist = "CURRENT_ONLY"
            # per-symbol court verdicts, if any exist
            cver = symbol_courts.get(f"{project}:{sym}")
            if cver:
                abi = cver.get("abi", "UNVERIFIED")
                sem = cver.get("semantic", "UNVERIFIED")
                own = cver.get("ownership", own)
                for cid, cdata in json.load(
                        open(SYMBOL_COURT_INDEX)).get("courts", {}).items():
                    if f"{project}:{sym}" in cdata.get("symbols", {}):
                        courts.append(cid)
            entry = {
                "entity_id": f"{project}:{sym}",
                "oracle_symbol": sym,
                "candidate_symbol": sym,
                "kind": "FUNC" if not data else "DATA",
                "export_status": "EXPORTED" if exported else "MISSING",
                "implementation_status": impl,
                "abi_status": abi,
                "semantic_status": sem,
                "ownership_status": own,
                "historical_status": hist,
                "stub_reason": reason,
                "courts": courts,
                "residuals": residuals,
            }
            entry["overall"] = derive_overall(entry)
            entries.append(entry)
        counts = {k: 0 for k in STATUS_MODEL["overall"]}
        dim = {k: {s: 0 for s in v} for k, v in STATUS_MODEL.items()
               if k != "overall" and k != "schema"}
        for e in entries:
            counts[e["overall"]] += 1
            for dname in ("export_status", "implementation_status",
                          "abi_status", "semantic_status",
                          "ownership_status", "historical_status"):
                dim[dname][e[dname]] += 1
        ledger["projects"][project] = {
            "oracle_dso": path,
            "candidate_dso": CANDIDATE_DSO,
            "oracle_functions": sum(1 for e in entries if e["kind"] == "FUNC"),
            "oracle_data": sum(1 for e in entries if e["kind"] == "DATA"),
            "candidate_functions": sum(1 for s, k in cand.items() if k in ("T", "t")),
            "counts": counts,
            "dimensions": dim,
            "obligations": entries,
        }
        print(f"{project}: funcs={ledger['projects'][project]['oracle_functions']} "
              f"data={ledger['projects'][project]['oracle_data']}")
        for k in ("MISSING", "STUB", "INTENTIONAL_NOOP", "IMPLEMENTED_UNVERIFIED",
                  "PARITY_VERIFIED", "DATA_EXPORTED", "DATA_MISSING"):
            if counts[k]:
                print(f"    {k}: {counts[k]}")
    out = os.path.join(ATLAS, "PARITY_OBLIGATIONS.json")
    with open(out, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("ledger ->", out)


if __name__ == "__main__":
    main()
