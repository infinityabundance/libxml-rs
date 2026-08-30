#!/usr/bin/env python3
"""ERROR-001 — differential court for the error semantics surface (11.1-M).

Compiles courts/suites/data-abi/error-family-probe.c against the system
libxml2 2.15.3 (oracle) and against the candidate headers + DSO and requires
byte-identical stdout. The probe runs every deterministic malformed/edge
input through four passes:

  1. default — no handlers; the library's default stderr output is captured
     by redirecting fd 2 and replayed (escaped) into stdout;
  2. struct  — a structured handler prints every xmlError field
     (domain/code/level/line/int1/int2/file/str1/str2/str3/msg);
  3. frag    — a generic handler prints every xmlFormatError fragment
     exactly as the library formats it;
  4. noerr   — XML_PARSE_NOERROR|XML_PARSE_NOWARNING suppression (stderr
     must be empty).

The corpus covers parser errors (unclosed/truncated tags, mismatched end
tags, extra content, doc-level invalid element names), entity/character
references (missing ';', no name, undeclared entity, out-of-bounds values),
attribute errors (unquoted values, missing values, duplicates, construct
errors), content errors (']]>', invalid Chars, invalid UTF-8 with the
"Bytes:" dump), markup errors (PI/comment/CDATA/XML-decl), the 80-column
source-window cap, UTF-8/caret placement, and filename variants.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/error_family_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "error-family-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
ORACLE_SO = "/usr/lib/libxml2.so.16"


def run(cmd):
    r = subprocess.run(cmd, capture_output=True)
    return r.returncode, r.stdout, r.stderr


def b2s(b):
    return b.decode("utf-8", "replace")


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "err-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "err-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED (rc=%d):\n%s" % (rc, b2s(err)[-2000:]))
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "err-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "err-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED (rc=%d):\n%s" % (rc, b2s(err)[-2000:]))
        return 1
    if err.strip():
        print("CANDIDATE LEAKED TO STDERR:\n" + b2s(err)[-2000:])
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
        "court": "ERROR-001",
        "phase": "11.1-M",
        "timestamp": ts,
        "probe": "courts/suites/data-abi/error-family-probe.c",
        "oracle": {"dso": ORACLE_SO, "stdout_sha256": o_hash},
        "candidate": {"dso": CAND_SO, "stdout_sha256": c_hash},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatches),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatches[:30]],
    }
    rpath = os.path.join(RECEIPTS, "error-family-" + ts.replace(":", "").replace("-", "") + ".json")
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
