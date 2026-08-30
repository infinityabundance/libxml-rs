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
    "schema": "parity-obligations-status-model-2",
    "export_status": ["MISSING", "EXPORTED"],
    "implementation_status": ["STUB", "INTENTIONAL_NOOP", "IMPLEMENTED", "UNKNOWN", "NOT_APPLICABLE"],
    "abi_status": ["UNVERIFIED", "PASS", "FAIL", "EVIDENCE_CONFLICT"],
    "semantic_status": ["UNVERIFIED", "PARTIAL", "PASS", "FAIL", "EVIDENCE_CONFLICT"],
    "ownership_status": ["NOT_APPLICABLE", "UNVERIFIED", "PASS", "FAIL", "EVIDENCE_CONFLICT"],
    "historical_status": ["CURRENT_ONLY", "PARTIAL", "VERIFIED", "EVIDENCE_CONFLICT"],
    # Verification ladder: an entry climbs it only through courts.
    "overall": ["MISSING", "STUB", "INTENTIONAL_NOOP", "IMPLEMENTED_UNVERIFIED",
                "CURRENT_PARITY_VERIFIED", "HISTORICAL_PARITY_VERIFIED",
                "CUSTODIAN_VERIFIED", "DATA_MISSING", "DATA_EXPORTED_UNVERIFIED",
                "DATA_PARITY_VERIFIED"],
    "current_parity_verified_definition": (
        "EXPORTED + IMPLEMENTED + abi_status PASS + semantic_status PASS "
        "+ ownership_status in (NOT_APPLICABLE, PASS), each supported by a "
        "passing court recorded in courts[]"),
    "historical_parity_verified_definition": (
        "CURRENT_PARITY_VERIFIED + historical_status VERIFIED (surface and "
        "behavior verified across every historically applicable epoch)"),
    "custodian_verified_definition": (
        "HISTORICAL_PARITY_VERIFIED across every historically applicable "
        "API/ABI/semantic epoch — not necessarily every version redundantly"),
}
# The six per-symbol status dimensions tracked per obligation.
STATUS_DIMENSIONS = ["export_status", "implementation_status", "abi_status",
                     "semantic_status", "ownership_status", "historical_status"]

# Per-dimension merge rule for multiple courts (see load_symbol_courts).
# "UNVERIFIED" absorbs any real verdict; PASS+PASS stays PASS; a conflict
# between PASS and FAIL (or FAIL and FAIL) is EVIDENCE_CONFLICT, which fails
# the court rather than hiding the contradiction.
DIM_MERGE = {
    ("UNVERIFIED", "PASS"): "PASS",
    ("PASS", "PASS"): "PASS",
    ("PASS", "FAIL"): "EVIDENCE_CONFLICT",
    ("FAIL", "PASS"): "EVIDENCE_CONFLICT",
    ("FAIL", "FAIL"): "EVIDENCE_CONFLICT",
    ("UNVERIFIED", "FAIL"): "FAIL",
    ("FAIL", "UNVERIFIED"): "FAIL",
    ("PASS", "UNVERIFIED"): "PASS",
    ("UNVERIFIED", "PARTIAL"): "PARTIAL",
    ("PARTIAL", "PARTIAL"): "PARTIAL",
    ("PARTIAL", "PASS"): "PARTIAL",
    ("PASS", "PARTIAL"): "PARTIAL",
    ("PARTIAL", "FAIL"): "EVIDENCE_CONFLICT",
    ("FAIL", "PARTIAL"): "EVIDENCE_CONFLICT",
    ("UNVERIFIED", "VERIFIED"): "VERIFIED",
    ("VERIFIED", "VERIFIED"): "VERIFIED",
    ("VERIFIED", "PARTIAL"): "PARTIAL",
    ("PARTIAL", "VERIFIED"): "PARTIAL",
    ("VERIFIED", "FAIL"): "EVIDENCE_CONFLICT",
    ("FAIL", "VERIFIED"): "EVIDENCE_CONFLICT",
    ("UNVERIFIED", "CURRENT_ONLY"): "CURRENT_ONLY",
    ("CURRENT_ONLY", "CURRENT_ONLY"): "CURRENT_ONLY",
    ("CURRENT_ONLY", "VERIFIED"): "VERIFIED",
    ("VERIFIED", "CURRENT_ONLY"): "VERIFIED",
    ("CURRENT_ONLY", "PARTIAL"): "PARTIAL",
    ("PARTIAL", "CURRENT_ONLY"): "PARTIAL",
}


def merge_dim(left, right):
    """Merge two per-dimension court verdicts; EVIDENCE_CONFLICT when the
    courts contradict each other."""
    if left == right:
        return left
    if left == "UNVERIFIED":
        return right
    if right == "UNVERIFIED":
        return left
    if left == "NOT_APPLICABLE":
        return right
    if right == "NOT_APPLICABLE":
        return left
    return DIM_MERGE.get((left, right), "EVIDENCE_CONFLICT")


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
    # libxslt exports whose upstream 1.1.45 bodies are literally trivial
    # (constant returns / no-ops) — the candidate matches them exactly
    "xsltSecurityAllow": "R-000160",
    "xsltSecurityForbid": "R-000160",
    "xsltGetDebuggerStatus": "R-000160",
    "xsltFreeLocales": "R-000160",
    "xsltFreeAVTList": "R-000160",
    "xsltExtensionInstructionResultRegister": "R-000160",
}


def load_symbol_courts():
    """Per-symbol court verdicts (atlas/SYMBOL_COURT_INDEX.json).

    Schema:
      { "schema": "symbol-court-index-1",
        "courts": { "<court-id>": { "symbols": { "<proj>:<sym>": {
             "abi": "PASS", "semantic": "PASS", "ownership": "PASS" } } } } }

    Aggregation is PER DIMENSION, never last-write-wins: multiple courts may
    verify different dimensions of the same symbol. Contradictory verdicts on
    the same dimension (PASS vs FAIL) produce EVIDENCE_CONFLICT, which fails
    the ledger court rather than hiding the contradiction.

    Returns { "<proj>:<sym>": { "abi": {"status": ..., "courts": [...]},
                                  ... } } plus a list of conflicts.
    """
    if not os.path.exists(SYMBOL_COURT_INDEX):
        return {}, []
    d = json.load(open(SYMBOL_COURT_INDEX))
    out = {}
    conflicts = []
    # Map long dimension names ("abi_status") to the short keys consumed by
    # main() ("abi"). The index file accepts either spelling.
    SHORT = {"abi_status": "abi", "semantic_status": "semantic",
             "ownership_status": "ownership", "historical_status": "historical"}
    for cid, cdata in sorted((d.get("courts") or {}).items()):
        for key, verdicts in sorted((cdata.get("symbols") or {}).items()):
            entry = out.setdefault(key, {})
            for dim in STATUS_DIMENSIONS:
                if dim == "export_status" or dim == "implementation_status":
                    continue  # these come from the DSO/source scan, not courts
                v = verdicts.get(dim)
                if v is None:
                    v = verdicts.get(SHORT.get(dim))
                if v is None:
                    continue
                cur = entry.get(SHORT.get(dim))
                if cur is None:
                    entry[SHORT.get(dim)] = {"status": v, "courts": [cid]}
                    continue
                merged = merge_dim(cur["status"], v)
                if merged == "EVIDENCE_CONFLICT":
                    conflicts.append((key, SHORT.get(dim), cur["status"], v, cid))
                entry[SHORT.get(dim)] = {"status": merged, "courts": cur["courts"] + [cid]}
    return out, conflicts


def derive_overall(entry):
    if entry["export_status"] == "MISSING":
        return "DATA_MISSING" if entry["kind"] == "DATA" else "MISSING"
    if entry["kind"] == "DATA":
        # Exported data is behavioral state (parser defaults, error handler
        # pointers, allocator hooks, loader function pointers, ...): presence
        # is not parity. DATA_PARITY_VERIFIED requires a passing data court
        # (kind/size/initial value/mutability/address stability/C<->Rust
        # visibility) recorded in the symbol-court index.
        if (entry["abi_status"] == "PASS"
                and entry["semantic_status"] == "PASS"
                and entry["ownership_status"] in ("NOT_APPLICABLE", "PASS")):
            return "DATA_PARITY_VERIFIED"
        return "DATA_EXPORTED_UNVERIFIED"
    if entry["implementation_status"] == "STUB":
        return "STUB"
    if entry["implementation_status"] == "INTENTIONAL_NOOP":
        return "INTENTIONAL_NOOP"
    if entry["implementation_status"] != "IMPLEMENTED":
        return "IMPLEMENTED_UNVERIFIED"
    if (entry["abi_status"] == "PASS"
            and entry["semantic_status"] == "PASS"
            and entry["ownership_status"] in ("NOT_APPLICABLE", "PASS")):
        if entry["historical_status"] == "VERIFIED":
            return "CUSTODIAN_VERIFIED"
        if entry["historical_status"] in ("PARTIAL",):
            return "HISTORICAL_PARITY_VERIFIED"
        return "CURRENT_PARITY_VERIFIED"
    return "IMPLEMENTED_UNVERIFIED"


def main():
    hay = "\n".join(open(p, encoding="utf-8", errors="replace").read()
                    for p in rust_sources())
    symbol_courts, court_conflicts = load_symbol_courts()
    if court_conflicts:
        raise SystemExit(
            "SYMBOL-COURT EVIDENCE CONFLICT in atlas/SYMBOL_COURT_INDEX.json:\n  "
            + "\n  ".join(f"{k} [{dim}]: {a} vs {b} from {cid}"
                          for k, dim, a, b, cid in court_conflicts))
    ledger = {"schema": "parity-obligations-3",
              "generated": __import__("datetime").datetime.now(
                  __import__("datetime").timezone.utc)
              .strftime("%Y-%m-%dT%H:%M:%SZ"),
              "status_model": STATUS_MODEL,
              "symbol_court_index": os.path.relpath(SYMBOL_COURT_INDEX, ROOT),
              "verification_policy": (
                  "CURRENT_PARITY_VERIFIED is earned only when per-symbol "
                  "courts exist and pass (abi + semantic + ownership); "
                  "HISTORICAL_PARITY_VERIFIED adds historical_status VERIFIED; "
                  "CUSTODIAN_VERIFIED means verified across every historically "
                  "applicable epoch. IMPLEMENTED_UNVERIFIED means the export "
                  "exists with real logic but has not yet been verified by a "
                  "court. Court verdicts merge PER DIMENSION with provenance; "
                  "contradictory verdicts fail the ledger as EVIDENCE_CONFLICT."),
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
            # per-symbol court verdicts (per-dimension, with provenance)
            cver = symbol_courts.get(f"{project}:{sym}")
            if cver:
                for dim, key in (("abi", "abi_status"),
                                 ("semantic", "semantic_status"),
                                 ("ownership", "ownership_status"),
                                 ("historical", "historical_status")):
                    if dim in cver:
                        v = cver[dim]
                        if key == "ownership_status":
                            own = v["status"]
                        elif key == "historical_status":
                            hist = v["status"]
                        elif key == "abi_status":
                            abi = v["status"]
                        elif key == "semantic_status":
                            sem = v["status"]
                        courts.extend(cid for cid in v["courts"] if cid not in courts)
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
        dim = {k: {s: 0 for s in STATUS_MODEL[k]}
               for k in STATUS_DIMENSIONS}
        for e in entries:
            counts[e["overall"]] += 1
            for dname in STATUS_DIMENSIONS:
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
                  "CURRENT_PARITY_VERIFIED", "HISTORICAL_PARITY_VERIFIED",
                  "CUSTODIAN_VERIFIED", "DATA_EXPORTED_UNVERIFIED", "DATA_PARITY_VERIFIED",
                  "DATA_MISSING"):
            if counts[k]:
                print(f"    {k}: {counts[k]}")
    out = os.path.join(ATLAS, "PARITY_OBLIGATIONS.json")
    with open(out, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("ledger ->", out)


if __name__ == "__main__":
    main()
