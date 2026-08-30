#!/usr/bin/env python3
"""DATA-GLOBALS-001 — differential court for the exported libxml2 data globals
and the chvalid xmlIs* family (11.1-G/I data-ABI closure, residual R-000135).

Compiles courts/suites/data-abi/data-globals-probe.c twice:

  1. oracle  — against the system libxml2 (/usr/include/libxml2, -lxml2)
  2. candidate — against the candidate headers (include/) and DSO
               (target/debug/liblibxml_rs.so)

and requires the two program outputs to be byte-identical. The probe is
deterministic: it prints the pubid table, every range group's ranges, the
SAX handler slot patterns (NULL/non-NULL only, never addresses), the
xmlLastError initial state, and FNV-1a hashes of the xmlIs* functions over
the whole BMP plus supplementary-plane samples.

Receipts are written to courts/receipts/phase-11/ and survive deletion of
target/.

Usage:
    python3 tools/abi/data_globals_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "data-globals-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
ORACLE_SO = "/usr/lib/libxml2.so.16"
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    for f in (PROBE, ORACLE_SO, CAND_SO):
        if not os.path.exists(f):
            print(f"MISSING {f}")
            return 1

    # ---- oracle build (system headers) ------------------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "dg-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "dg-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + err[-2000:])
        return 1

    # ---- candidate build (candidate headers + DSO) ------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "dg-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "dg-cand")])
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

    receipt = {
        "court": "DATA-GLOBALS-001",
        "phase": "11.1-I",
        "timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "probe": "courts/suites/data-abi/data-globals-probe.c",
        "oracle": {"dso": ORACLE_SO, "stdout_sha256": oracle_hash,
                   "line_count": len(o_out.splitlines())},
        "candidate": {"dso": CAND_SO, "stdout_sha256": cand_hash,
                      "line_count": len(c_out.splitlines())},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatch_lines),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatch_lines[:50]],
    }
    ts = receipt["timestamp"].replace(":", "").replace("-", "").replace("Z", "Z")
    rpath = os.path.join(RECEIPTS, f"data-globals-{ts}.json")
    with open(rpath, "w") as f:
        json.dump(receipt, f, indent=1)
    print(f"receipt -> {rpath}")
    print(f"oracle stdout sha256   {oracle_hash}")
    print(f"candidate stdout sha256 {cand_hash}")
    print(f"byte-identical={identical} mismatch_lines={len(mismatch_lines)} "
          f"verdict={receipt['verdict']}")
    for ln, a, b in mismatch_lines[:10]:
        print(f"  line {ln}: oracle={a!r} candidate={b!r}")
    return 0 if identical else 1


if __name__ == "__main__":
    sys.exit(main())
