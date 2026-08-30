#!/usr/bin/env python3
"""GLOBALS-001 / THREADING-001 — differential court for global state,
initialization lifecycle and concurrency (11.1-K).

Compiles courts/suites/data-abi/globals-threading-probe.c against the system
libxml2 2.15.3 and against the candidate headers + DSO and requires
byte-identical stdout. The probe covers:

  1. init lifecycle (xmlInitParser repeat, xmlCleanupParser,
     reinitialization, xmlInitThreads);
  2. mutation of the exported parser-default globals (save/restore);
  3. function-pointer global replacement (xmlSetGenericErrorFunc counting
     handler — the raise must stream the xmlFormatError fragments: err-count 6);
  4. concurrent parsing across 8 threads (FNV-1a digests, sorted);
  5. concurrent init/cleanup across 4 threads while parsing.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/globals_threading_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "globals-threading-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
ORACLE_SO = "/usr/lib/libxml2.so.16"


def run(cmd):
    # capture_output without text=True keeps stdout bytes for exact hashing.
    r = subprocess.run(cmd, capture_output=True)
    return r.returncode, r.stdout, r.stderr


def b2s(b):
    return b.decode("utf-8", "replace")


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    rc, _, err = run(["gcc", "-std=c11", "-w", "-O2", "-pthread",
                      "-o", os.path.join(ROOT, "target", "glob-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, o_out, o_err = run([os.path.join(ROOT, "target", "glob-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED (rc=%d):\n%s" % (rc, b2s(o_err)[-2000:]))
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-O2", "-pthread",
                      "-o", os.path.join(ROOT, "target", "glob-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, c_out, c_err = run([os.path.join(ROOT, "target", "glob-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED (rc=%d):\n%s" % (rc, b2s(c_err)[-2000:]))
        return 1

    o_hash = hashlib.sha256(o_out).hexdigest()
    c_hash = hashlib.sha256(c_out).hexdigest()
    identical = o_hash == c_hash
    mismatches = []
    if not identical:
        o_lines, c_lines = o_out.splitlines(), c_out.splitlines()
        for i, (a, b) in enumerate(zip(o_lines, c_lines)):
            if a != b:
                mismatches.append((i + 1, b2s(a), b2s(b)))
        if len(o_lines) != len(c_lines):
            mismatches.append(("len", len(o_lines), len(c_lines)))
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    receipt = {
        "court": "GLOBALS-THREADING-001",
        "phase": "11.1-K",
        "timestamp": ts,
        "probe": "courts/suites/data-abi/globals-threading-probe.c",
        "oracle": {"dso": ORACLE_SO, "stdout_sha256": o_hash},
        "candidate": {"dso": CAND_SO, "stdout_sha256": c_hash},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatches),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatches[:30]],
    }
    rpath = os.path.join(RECEIPTS, "globals-threading-" + ts.replace(":", "").replace("-", "") + ".json")
    with open(rpath, "w") as f:
        json.dump(receipt, f, indent=1)
    print(f"receipt -> {rpath}")
    print(f"byte-identical={identical} mismatch_lines={len(mismatches)} "
          f"verdict={receipt['verdict']}")
    for ln, a, b in mismatches[:12]:
        print(f"  line {ln}: oracle={a!r} candidate={b!r}")
    return 0 if identical else 1


if __name__ == "__main__":
    sys.exit(main())
