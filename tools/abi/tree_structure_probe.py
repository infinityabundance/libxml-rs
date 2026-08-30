#!/usr/bin/env python3
"""TREE-001 — differential court for the parser/tree structural surface (11.1-N).

Compiles courts/suites/data-abi/tree-structure-probe.c against the system
libxml2 2.15.3 (oracle) and against the candidate headers + DSO and requires
byte-identical stdout. The probe fingerprints a parsed document exactly as a C
consumer traversing xmlNode/xmlDoc/xmlAttr/xmlDtd/xmlEntity would observe it:

  - node type/name/line/extra/doc-ptr/ns, child/parent/next/prev invariants,
    xmlGetLineNo and element-sibling/base/content API checks;
  - nsDef bindings (default and prefixed, xmlns="");
  - attribute representation (value text children, atype, ns);
  - entity-reference nodes (content shared with the entity, child list
    pointing at the entity declaration) and lazily parsed entity content;
  - DTD construction (intSubset in the doc child chain, declaration hash
    tables created lazily, decl nodes as DTD children);
  - document fields (properties, parseFlags, standalone incl. the -2 XML-decl
    case, compression, dict refcounts, ids/refs tables);
  - options variants (NOBLANKS, COMPACT, DTDATTR, NOENT|DTDLOAD, RECOVER,
    HUGE, BIG_LINES) and copy/unlink/relink/set-prop mutation checks.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/tree_structure_probe.py
"""

import datetime
import hashlib
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "tree-structure-probe.c")
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
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "tree-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, o_out, o_err = run([os.path.join(ROOT, "target", "tree-oracle")])
    if rc != 0:
        print("ORACLE RUN FAILED (rc=%d):\n%s" % (rc, b2s(o_err)[-2000:]))
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "tree-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"), "-llibxml_rs",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    rc, c_out, _c_err = run([os.path.join(ROOT, "target", "tree-cand")])
    if rc != 0:
        print("CANDIDATE RUN FAILED (rc=%d):\n%s" % (rc, b2s(_c_err)[-2000:]))
        return 1
    # The probe captures the default-handler stderr diagnostics inside
    # parse_and_dump and replays them (escaped) into stdout, so the stdout
    # comparison below already covers parser warnings/errors byte-for-byte.

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
        "court": "TREE-001",
        "phase": "11.1-N",
        "timestamp": ts,
        "probe": "courts/suites/data-abi/tree-structure-probe.c",
        "oracle": {"dso": ORACLE_SO, "stdout_sha256": o_hash},
        "candidate": {"dso": CAND_SO, "stdout_sha256": c_hash},
        "verdict": "PASS" if identical else "FAIL",
        "mismatch_count": len(mismatches),
        "mismatches": [{"line": ln, "oracle": a, "candidate": b}
                       for ln, a, b in mismatches[:30]],
    }
    rpath = os.path.join(RECEIPTS, "tree-structure-" + ts.replace(":", "").replace("-", "") + ".json")
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
