#!/usr/bin/env python3
"""11.1-W — Canonical evidence regeneration entrypoint.

Every machine-readable ledger committed under atlas/ is regenerated from this
single entrypoint in dependency order, so that every headline total is
reproducible ("never manually type the headline counts" — 11.1-W).

Steps (each is a generator with a committed artifact set):

  1. surface_reconcile      atlas/SURFACE_RECONCILIATION.{json,md}
  2. parity_matrix          atlas/PARITY_MATRIX.{json,md},
                            atlas/API_PARITY_LEDGER.{json,md},
                            atlas/DOXYGEN_SURFACE_ATLAS.{json,md}
  3. surface_delta_engine   atlas/HISTORICAL_SURFACE_EPOCHS.{json,md}
  4. abi_probe_gen          atlas/ABI_PARITY_LEDGER.{json,md}
  5. parity_obligations     atlas/PARITY_OBLIGATIONS.{json,md}
  6. subsystem_census       atlas/SUBSYSTEM_CENSUS.{json,md}
  7. ledger_gen gen         atlas/RESIDUAL_LEDGER.md  (JSON is the
                            hand-maintained source of truth)

Upstream inputs that are themselves extracted by earlier phase tooling
(Doxygen inventories under oracle/historical/doxygen/, the Clang AST atlas
under atlas/api/, the oracle DSOs) are hashed and recorded as input identity
in the receipt rather than silently re-extracted; use --with-doxygen to
re-run the Doxygen census + apiatlas extraction pipeline first (expensive).

Usage:
  generate_all.py            regenerate every ledger + markdown view
  generate_all.py --check    regenerate into a temp dir and compare sha256
                             with the committed artifacts (reproducibility
                             court); exits 1 on any drift
  generate_all.py --with-doxygen   also re-run doxygen census + apiatlas
  generate_all.py --no-abi   skip the heavy gcc ABI probe (abi_probe_gen)
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TOOLS = os.path.join(ROOT, "tools")
ATLAS = os.path.join(ROOT, "atlas")
PY = sys.executable

# step name -> (script, artifacts)
STEPS = [
    ("surface_reconcile", "tools/archaeology/surface_reconcile.py",
     ["atlas/SURFACE_RECONCILIATION.json", "atlas/SURFACE_RECONCILIATION.md"]),
    ("parity_matrix", "tools/evidence/parity_matrix.py",
     ["atlas/PARITY_MATRIX.json", "atlas/PARITY_MATRIX.md",
      "atlas/API_PARITY_LEDGER.json", "atlas/API_PARITY_LEDGER.md",
      "atlas/DOXYGEN_SURFACE_ATLAS.json", "atlas/DOXYGEN_SURFACE_ATLAS.md"]),
    ("surface_delta_engine", "tools/evidence/surface_delta_engine.py",
     ["atlas/HISTORICAL_SURFACE_EPOCHS.json", "atlas/HISTORICAL_SURFACE_EPOCHS.md"]),
    ("abi_probe_gen", "tools/abi/abi_probe_gen.py",
     ["atlas/ABI_PARITY_LEDGER.json", "atlas/ABI_PARITY_LEDGER.md"]),
    ("parity_obligations", "tools/abi/parity_obligations.py",
     ["atlas/PARITY_OBLIGATIONS.json", "atlas/PARITY_OBLIGATIONS.md"]),
    ("subsystem_census", "tools/evidence/subsystem_census.py",
     ["atlas/SUBSYSTEM_CENSUS.json", "atlas/SUBSYSTEM_CENSUS.md"]),
    ("readme_counts", "tools/evidence/readme_counts.py", ["README.md"]),
    ("ledger_gen", "tools/evidence/ledger_gen.py",
     ["atlas/RESIDUAL_LEDGER.md"]),
]

# Upstream input identities recorded in the receipt (not regenerated here).
INPUT_IDENTITY = [
    ("oracle doxygen inventories",
     "oracle/historical/doxygen/*/inventory-*.json"),
    ("clang AST atlas", "atlas/api/**/*.json"),
    ("oracle DSO libxml2", "/usr/lib/libxml2.so.16"),
    ("oracle DSO libxslt", "/usr/lib/libxslt.so.1"),
    ("oracle DSO libexslt", "/usr/lib/libexslt.so.0"),
    ("candidate DSO", "target/debug/liblibxml_rs.so"),
    ("candidate SONAME libxml2", "target/debug/libxml2.so.16"),
    ("candidate SONAME libxslt", "target/debug/libxslt.so.1"),
    ("candidate SONAME libexslt", "target/debug/libexslt.so.0"),
    ("candidate headers", "include"),
    ("residual ledger JSON (hand-maintained)", "atlas/RESIDUAL_LEDGER.json"),
    ("symbol court index", "atlas/SYMBOL_COURT_INDEX.json"),
]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def tool_versions():
    def run(cmd):
        try:
            r = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
            return (r.stdout or r.stderr).strip().splitlines()[0][:120]
        except Exception:
            return None
    return {
        "python": run([PY, "--version"]),
        "doxygen": run(["doxygen", "--version"]),
        "gcc": run(["gcc", "--version"]),
        "clang": run(["clang", "--version"]),
        "nm": run(["nm", "--version"]),
        "readelf": run(["readelf", "--version"]),
        "git": run(["git", "--version"]),
    }


def git_head():
    try:
        r = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                           capture_output=True, text=True)
        if r.returncode == 0:
            return r.stdout.strip()
    except Exception:
        pass
    return None


def step_artifacts(step):
    return [os.path.join(ROOT, a) for a in step[2]]


def run_step(step, check_dir=None):
    name, script, arts = step
    cmd = [PY, os.path.join(ROOT, script)]
    if name == "ledger_gen" and check_dir is None:
        cmd.append("gen")
    if name == "readme_counts" and check_dir is not None:
        # Phase-14 evidence amendment: the README's generated sections join the
        # reproducibility gate (readme_counts.py --check writes nothing and
        # fails on drift).
        cmd.append("--check")
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT)
    detail = (r.stdout or "") + (r.stderr or "")
    if r.returncode != 0:
        return {"step": name, "ok": False, "detail": detail[-2000:],
                "hashes": None}
    hashes = {}
    for a in arts:
        p = os.path.join(ROOT, a)
        if not os.path.exists(p):
            return {"step": name, "ok": False,
                    "detail": f"missing artifact {a}\n{detail[-1500:]}",
                    "hashes": None}
        hashes[a] = sha256(p)
    return {"step": name, "ok": True, "detail": detail[-800:], "hashes": hashes}


def input_identity_hashes():
    """Hash every claimed input. The pattern may be a literal file, a
    directory (hashed recursively), or a glob (aggregate hash of the matching
    files). The 11.1-Z.1 fix: glob metacharacters decide the branch, so
    `oracle/historical/doxygen/*/inventory-*.json` is actually expanded and
    the `include/` directory is hashed recursively instead of falling into
    the glob branch and yielding null."""
    import glob
    out = {}
    for label, pat in INPUT_IDENTITY:
        if pat.startswith("/usr/lib/"):
            p = pat
            out[label] = sha256(p) if os.path.exists(p) else None
            continue
        p = os.path.join(ROOT, pat)
        if any(ch in pat for ch in "*?["):
            files = sorted(f for f in glob.glob(p, recursive=True)
                           if os.path.isfile(f))
            h = hashlib.sha256()
            for f in files:
                h.update(os.path.relpath(f, ROOT).encode())
                h.update(b"\0")
                h.update(sha256(f).encode())
                h.update(b"\0")
            out[label] = {"files": len(files),
                          "aggregate_sha256": h.hexdigest()} if files else None
        elif os.path.isdir(p):
            out[label] = dir_hash(p)
        elif os.path.isfile(p):
            out[label] = sha256(p)
        else:
            out[label] = None
    return out


def dir_hash(path):
    h = hashlib.sha256()
    for root, _dirs, files in sorted(os.walk(path)):
        for fn in sorted(files):
            full = os.path.join(root, fn)
            rel = os.path.relpath(full, path)
            try:
                data = open(full, "rb").read()
            except OSError:
                continue
            h.update(rel.encode())
            h.update(b"\0")
            h.update(data)
            h.update(b"\0")
    return h.hexdigest()


def build_receipt(results, input_hash, versions, head, check=False):
    return {
        "schema": "evidence-generation-receipt-1",
        "phase": "11.1-W",
        "mode": "check" if check else "generate",
        "candidate_git_head": head,
        "tools": versions,
        "inputs": input_hash,
        "steps": results,
        "verdict": "PASS" if all(r["ok"] for r in results) else "FAIL",
    }


def write_receipt(receipt):
    out = os.path.join(ATLAS, "EVIDENCE_GENERATION_RECEIPT.json")
    with open(out, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print("receipt ->", out)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="regenerate and byte-compare against committed artifacts")
    ap.add_argument("--with-doxygen", action="store_true",
                    help="also re-run the doxygen census + apiatlas extraction")
    ap.add_argument("--no-abi", action="store_true",
                    help="skip the heavy gcc ABI probe step")
    args = ap.parse_args()

    if args.with_doxygen:
        r = subprocess.run([PY, os.path.join(TOOLS, "archaeology",
                                             "run_doxygen_census.py"), "--profiles", "public,full"],
                           cwd=ROOT, capture_output=True, text=True)
        if r.returncode != 0:
            print("doxygen census failed:\n", (r.stdout + r.stderr)[-1500:])
            return 1
        for proj, ver in (("libxml2", "2.15.3"), ("libxslt", "1.1.45"),
                          ("libexslt", "0.8.25")):
            tag = {"libxml2": "v2.15.3", "libxslt": "v1.1.45",
                   "libexslt": "v1.1.45"}[proj]
            subprocess.run([PY, os.path.join(TOOLS, "archaeology", "apiatlas.py"),
                            proj, ver, tag], cwd=ROOT, capture_output=True, text=True)
            subprocess.run([PY, os.path.join(TOOLS, "archaeology",
                                             "doxygen_profile.py"), "gen", proj, "system", "public"],
                           cwd=ROOT, capture_output=True, text=True)
            subprocess.run([PY, os.path.join(TOOLS, "archaeology",
                                             "doxygen_inventory.py"), "run", proj, "system", "public"],
                           cwd=ROOT, capture_output=True, text=True)

    check_dir = tempfile.mkdtemp(prefix="generate-all-") if args.check else None
    # In check mode, snapshot the committed artifact hashes FIRST, regenerate,
    # then byte-compare — proving the committed evidence is reproducible.
    committed = {}
    if args.check:
        for step in STEPS:
            if args.no_abi and step[0] == "abi_probe_gen":
                continue
            for a in step[2]:
                p = os.path.join(ROOT, a)
                committed[a] = sha256(p) if os.path.exists(p) else None
    results = []
    for step in STEPS:
        if args.no_abi and step[0] == "abi_probe_gen":
            continue
        print(f"── {step[0]} ──")
        res = run_step(step)
        results.append(res)
        print(("ok" if res["ok"] else "FAILED") + ("\n" + res["detail"] if not res["ok"] else ""))
        if not res["ok"]:
            print(res["detail"][-1200:])
            break

    receipt = build_receipt(results, input_identity_hashes(), tool_versions(),
                            git_head(), check=args.check)
    write_receipt(receipt)

    if args.check:
        drift = []
        for r in results:
            if not r["ok"] or not r["hashes"]:
                drift.append(r["step"])
                continue
            for a, h in r["hashes"].items():
                if committed.get(a) is None:
                    drift.append(f"{a}: missing before regeneration")
                elif committed[a] != h:
                    drift.append(f"{a}: regenerated differs from committed")
        if drift:
            print("REPRODUCIBILITY FAILED:")
            for d in drift:
                print("  ", d)
            return 1
        print("reproducibility: all artifacts byte-identical to committed state")
    return 0 if all(r["ok"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
