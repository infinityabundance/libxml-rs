#!/usr/bin/env python3
"""ENCODING-001 — differential court for the xmlCharEncoding handler family
(xmlLookupCharEncodingHandler, xmlGetCharEncodingHandler,
 xmlOpenCharEncodingHandler, xmlCreateCharEncodingHandler,
 xmlCharEncNewCustomHandler, xmlGetCharEncodingName).

Compiles courts/suites/data-abi/encoding-family-probe.c against the system
libxml2 2.15.3 and against the candidate headers + DSO and requires
byte-identical stdout. Receipts written to courts/receipts/phase-11/.

Scope note: the probe covers the native converter set (UTF-8, UTF-16LE/BE,
UTF-16, ISO-8859-1, US-ASCII) plus every error path. Encodings that upstream
serves through iconv/ICU (UCS-4*, EBCDIC, UCS-2, ISO-8859-2..9/10/11/13..16,
ISO-2022-JP, Shift_JIS, EUC-JP, windows-1252) and the HTML static handler are
documented candidate divergences (no iconv backend; residual R-000157) and are
deliberately excluded.

Usage:
    python3 tools/abi/encoding_family_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "encoding-family-probe.c")
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
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "enc-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "enc-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "enc-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "enc-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED:\n" + b2s(err)[-2000:])
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
        "court": "ENCODING-001",
        "phase": "11.1-I",
        "timestamp": ts,
        "probe": "courts/suites/data-abi/encoding-family-probe.c",
        "oracle": {"dso": ORACLE_SO, "stdout_sha256": o_hash},
        "candidate": {"dso": CAND_SO, "stdout_sha256": c_hash},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatches),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatches[:30]],
    }
    rpath = os.path.join(RECEIPTS, "encoding-family-" + ts.replace(":", "").replace("-", "") + ".json")
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
