#!/usr/bin/env python3
"""ALLOCATOR-HOOK-001 — differential court for the allocator-hook contract
(11.1-Z.2, R-000176).

Compiles courts/suites/data-abi/allocator-hook-probe.c twice:

  1. oracle  — against the system libxml2 (/usr/include/libxml2, -lxml2)
  2. candidate — against the candidate headers (include/) and DSO
               (target/debug/liblibxml_rs.so)

and requires the two program outputs to be byte-identical. The probe prints
only relative facts (return codes, NULL patterns, hook-observation flags,
exported-variable equality), so the two runs must match exactly.

This court proves the single-source-of-truth allocator model: xmlMemSetup
writes the exported variables, xmlMemGet reads them, and actual allocations
(through xmlMallocImpl et al.) observe the override — plus the direct
public-variable-assignment path and the NULL-hook rejection contract.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/allocator_hook_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "allocator-hook-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    for f in (PROBE, CAND_SO):
        if not os.path.exists(f):
            print(f"MISSING {f}")
            return 1

    # ---- oracle build (system headers) ------------------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "ah-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "ah-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + err[-2000:])
        return 1

    # ---- candidate build (candidate headers + DSO) ------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "ah-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "ah-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED:\n" + err[-2000:])
        return 1

    oracle_hash = hashlib.sha256(o_out.encode()).hexdigest()
    cand_hash = hashlib.sha256(c_out.encode()).hexdigest()
    identical = oracle_hash == cand_hash

    mismatch_lines = []
    if not identical:
        o_lines, c_lines = o_out.splitlines(), c_out.splitlines()
        for i, (a, b) in enumerate(zip(o_lines, c_lines)):
            if a != b:
                mismatch_lines.append((i + 1, a, b))
        if len(o_lines) != len(c_lines):
            mismatch_lines.append(("len", len(o_lines), len(c_lines)))

    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "ALLOCATOR-HOOK",
        "phase": "11.1-Z.2",
        "timestamp": ts,
        "schema": "allocator-hook-differential-1",
        "probe": os.path.relpath(PROBE, ROOT),
        "oracle_sha256": oracle_hash,
        "candidate_sha256": cand_hash,
        "identical": identical,
        "mismatches": mismatch_lines[:40],
        "verdict": "PASS" if identical else "FAIL",
    }
    rp = os.path.join(RECEIPTS, f"allocator-hook-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"oracle={oracle_hash} candidate={cand_hash} identical={identical}")
    if not identical:
        print("--- first mismatches:")
        for ln, a, b in mismatch_lines[:20]:
            print(f"  L{ln}: oracle  {a!r}\n       candidate {b!r}")
    return 0 if identical else 1


if __name__ == "__main__":
    sys.exit(main())
