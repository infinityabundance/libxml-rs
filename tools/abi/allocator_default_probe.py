#!/usr/bin/env python3
"""ALLOCATOR-DEFAULT-001 — differential court for the DEFAULT allocator
contract (11.1-Z.3, R-000178).

Compiles courts/suites/data-abi/allocator-default-probe.c twice:

  1. oracle  — against the system libxml2 (/usr/include/libxml2, -lxml2)
  2. candidate — against the candidate headers (include/) and DSO
               (target/debug/liblibxml_rs.so)

and requires the two program outputs to be byte-identical. The probe prints
only relative facts (NULL patterns, content preservation, routing counters,
and the exact xmlMemSize/xmlMemUsed/xmlMemBlocks values, which must be 0
under the default allocator on both sides), so the two runs must match
exactly.

This court proves the R-000178 default-allocator contract: the candidate's
default hooks are plain libc malloc/realloc/free/strdup wrappers (the
pre-Z.3 implementation used Rust `std::alloc` with fabricated layouts —
invalid-layout UB) with observable behavior identical to the oracle's
globals.c defaults (`xmlMalloc = malloc` etc.):
  - malloc(0) non-NULL, malloc(SIZE_MAX) NULL;
  - realloc grow/shrink preserve content, realloc(p,0) frees+returns NULL,
    realloc(NULL,n) allocates, realloc failure leaves the old block;
  - strdup content + strdup(NULL) NULL; free(NULL) no-op;
  - 100k alloc/free churn;
  - direct exported-variable assignment routes allocations;
  - xmlMemUsed/xmlMemBlocks/xmlMemSize stay 0 (upstream debug counters are
    maintained only by the debug allocator).

When valgrind is available, the candidate probe binary is additionally run
under valgrind (--error-exitcode) as a memory-safety sweep; the result is
recorded in the receipt but the verdict is the differential equality.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/allocator_default_probe.py
"""

import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "allocator-default-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_SO = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")


def run(cmd, timeout=120):
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout, r.stderr


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    for f in (PROBE, CAND_SO):
        if not os.path.exists(f):
            print(f"MISSING {f}")
            return 1

    # ---- oracle build (system headers) ------------------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "ad-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(ROOT, "target", "ad-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + err[-2000:])
        return 1

    # ---- candidate build (candidate headers + DSO) ------------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(ROOT, "target", "ad-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(ROOT, "target", "ad-cand")])
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

    # ---- optional valgrind sweep on the candidate (memory safety) ---------
    # NB: valgrind 3.25.1 in this environment SIGILLs inside the dynamic
    # loader (_dl_start) on every binary, including /bin/true — an
    # environment/toolchain incompatibility, not a probe failure. The court
    # records `usable: false` with the reason when that is observed.
    valgrind = shutil.which("valgrind")
    vg = None
    if valgrind:
        vg_rc, _, vg_err = run(
            [valgrind, "--error-exitcode=99", "--leak-check=full",
             "--errors-for-leak-kinds=definite,indirect", "-q",
             os.path.join(ROOT, "target", "ad-cand")],
            timeout=600,
        )
        loader_crash = "_dl_start" in vg_err and "SIGILL" in vg_err
        vg = {
            "available": True,
            "binary": valgrind,
            "error_exitcode": 99,
            "exit_code": vg_rc,
            "usable": not loader_crash and vg_rc == 0,
            "reason": "valgrind SIGILLs in the dynamic loader (_dl_start) on every "
            "binary in this environment (toolchain incompatibility); the "
            "memory-safety sweep is therefore not executable here"
            if loader_crash
            else None,
            "stderr_tail": vg_err[-2000:],
        }
    else:
        vg = {"available": False}

    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "ALLOCATOR-DEFAULT",
        "case": "ALLOCATOR-DEFAULT-001",
        "phase": "11.1-Z.3",
        "timestamp": ts,
        "schema": "allocator-default-differential-1",
        "probe": os.path.relpath(PROBE, ROOT),
        "oracle_sha256": oracle_hash,
        "candidate_sha256": cand_hash,
        "identical": identical,
        "mismatches": mismatch_lines[:40],
        "valgrind": vg,
        "verdict": "PASS" if identical else "FAIL",
    }
    rp = os.path.join(RECEIPTS, f"allocator-default-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"oracle={oracle_hash} candidate={cand_hash} identical={identical}")
    if valgrind:
        print(f"valgrind exit={vg['exit_code']} usable={vg['usable']}")
    if not identical:
        print("--- first mismatches:")
        for ln, a, b in mismatch_lines[:20]:
            print(f"  L{ln}: oracle  {a!r}\n       candidate {b!r}")
    return 0 if identical else 1


if __name__ == "__main__":
    sys.exit(main())
