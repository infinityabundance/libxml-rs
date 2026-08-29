#!/usr/bin/env python3
"""11.1-I function-level parity obligation ledger generator.

For every externally relevant function exported by the ORACLE DSOs
(system libxml2/libxslt), records a machine-readable parity obligation:

    entity_id, oracle_symbol, candidate_symbol (same for C ABI),
    exported (bool), stub (bool|None), category, courts[], residuals[],
    status (EXPORTED_VERIFIED / EXPORTED_STUB / MISSING)

The ledger is generated from the DSO symbol tables plus a source scan of
the candidate for stub bodies (`todo!`, `unimplemented!`, `panic!` on
valid C API paths, unconditional default returns). "Symbol exists" is
never treated as "function is implemented": EXPORTED_STUB entries are
residuals that must be closed.

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

    Returns a (status, reason) pair: status is one of
    'VERIFIED' (has real logic), 'STUB' (placeholder detected),
    'UNKNOWN' (no source body found / macro export).
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
    return "VERIFIED", "has logic"


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


def main():
    hay = "\n".join(open(p, encoding="utf-8", errors="replace").read()
                    for p in rust_sources())
    ledger = {"schema": "parity-obligations-1",
              "generated": __import__("datetime").datetime.utcnow()
              .strftime("%Y-%m-%dT%H:%M:%SZ"),
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
            is_func = kind.isupper() and kind in "TtWw" or kind in "Tt"
            data = not (kind in ("T", "t"))
            exported = sym in cand
            st, reason = None, None
            if exported and not data:
                st, reason = stub_score(sym, hay)
                if st in ("STUB", "UNKNOWN") and sym in DOCUMENTED_NOOPS:
                    st = "NOOP"
                    reason = f"documented intentional no-op (residual {DOCUMENTED_NOOPS[sym]})"
            if data:
                status = "DATA_EXPORTED" if exported else "DATA_MISSING"
            else:
                status = "EXPORTED_VERIFIED" if (exported and st == "VERIFIED") \
                    else "EXPORTED_NOOP" if (exported and st == "NOOP") \
                    else "EXPORTED_STUB" if exported else "MISSING"
            entries.append({
                "entity_id": f"{project}:{sym}",
                "oracle_symbol": sym,
                "candidate_symbol": sym,
                "kind": "FUNC" if not data else "DATA",
                "exported": exported,
                "stub": st,
                "stub_reason": reason,
                "status": status,
                "courts": [],
                "residuals": [DOCUMENTED_NOOPS[sym]] if sym in DOCUMENTED_NOOPS else [],
            })
        counts = {k: 0 for k in
                  ("MISSING", "EXPORTED_VERIFIED", "EXPORTED_STUB",
                   "EXPORTED_NOOP", "DATA_EXPORTED", "DATA_MISSING")}
        for e in entries:
            counts[e["status"]] += 1
        ledger["projects"][project] = {
            "oracle_dso": path,
            "candidate_dso": CANDIDATE_DSO,
            "oracle_functions": sum(1 for e in entries if e["kind"] == "FUNC"),
            "oracle_data": sum(1 for e in entries if e["kind"] == "DATA"),
            "candidate_functions": sum(1 for s, k in cand.items() if k in ("T", "t")),
            "counts": counts,
            "obligations": entries,
        }
        print(f"{project}: funcs={ledger['projects'][project]['oracle_functions']} "
              f"data={ledger['projects'][project]['oracle_data']} "
              f"verified={counts['EXPORTED_VERIFIED']} noop={counts['EXPORTED_NOOP']} "
              f"stub={counts['EXPORTED_STUB']} missing={counts['MISSING']} "
              f"data_exp={counts['DATA_EXPORTED']} data_missing={counts['DATA_MISSING']}")
    out = os.path.join(ATLAS, "PARITY_OBLIGATIONS.json")
    with open(out, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("ledger ->", out)


if __name__ == "__main__":
    main()
