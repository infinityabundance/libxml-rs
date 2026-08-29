#!/usr/bin/env python3
"""WRITER-001 — differential court for the xmlTextWriter* family closure
(11.1-I writer family).

Compiles courts/suites/data-abi/writer-family-probe.c against the system
libxml2 and against the candidate headers + DSO and requires byte-identical
output. Receipts written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/writer_family_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "writer-family-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r.returncode, r.stdout, r.stderr


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "wr-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "wr-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + err[-2000:])
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "wr-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "wr-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED:\n" + err[-2000:])
        return 1

    o_hash = hashlib.sha256(o_out.encode()).hexdigest()
    c_hash = hashlib.sha256(c_out.encode()).hexdigest()
    identical = o_hash == c_hash
    mismatches = []
    if not identical:
        o_lines, c_lines = o_out.splitlines(), c_out.splitlines()
        for i, (a, b) in enumerate(zip(o_lines, c_lines)):
            if a != b:
                mismatches.append((i + 1, a, b))
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    receipt = {
        "court": "WRITER-001",
        "phase": "11.1-I",
        "timestamp": ts,
        "probe": "courts/suites/data-abi/writer-family-probe.c",
        "oracle": {"dso": "/usr/lib/libxml2.so.2", "stdout_sha256": o_hash},
        "candidate": {"dso": CAND_SO, "stdout_sha256": c_hash},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatches),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatches[:30]],
    }
    rpath = os.path.join(RECEIPTS, "writer-family-" + ts.replace(":", "").replace("-", "") + ".json")
    with open(rpath, "w") as f:
        json.dump(receipt, f, indent=1)
    print(f"receipt -> {rpath}")
    print(f"byte-identical={identical} mismatch_lines={len(mismatches)} "
          f"verdict={receipt['verdict']}")
    for ln, a, b in mismatches[:8]:
        print(f"  line {ln}: oracle={a!r} candidate={b!r}")
    return 0 if identical else 1


if __name__ == "__main__":
    sys.exit(main())
