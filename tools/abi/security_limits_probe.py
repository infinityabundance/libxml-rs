#!/usr/bin/env python3
"""SECURITY-LIMITS — differential court for the security-relevant surface (11.1-V).

Compiles courts/suites/data-abi/security-limits-probe.c against the system
libxml2 2.15.3 (oracle) and against the candidate headers + DSO and requires
byte-identical stdout (plus exit code). The probe exercises, with
deterministic output (never attacker-controlled content):

  1. entity-expansion amplification (billion laughs) — bounded by default;
  2. the same document under XML_PARSE_HUGE — must parse;
  3. recursive entity loop — rejected;
  4. deep nesting — depth limit — rejected;
  5. the same deep document under XML_PARSE_HUGE — must parse;
  6. xmlCtxtSetMaxAmplification raising the factor — bound lifted;
  7. XML_PARSE_NONET — external network entity not fetched;
  8. external entity from a local file (XML_PARSE_NOENT);
  9. XInclude of a local document;
 10. catalog resolution via xmlLoadCatalog + xmlCatalogResolvePublic.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/security_limits_probe.py
"""

import datetime
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "security-limits-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")
CAND_DIR = os.path.join(ROOT, "target", "debug")
ORACLE_SO = "/usr/lib/libxml2.so.16"


def run(cmd):
    r = subprocess.run(cmd, capture_output=True)
    return r.returncode, r.stdout, r.stderr


def b2s(b):
    return b.decode("utf-8", "replace")


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "sec-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + b2s(err)[-2000:])
        sys.exit(1)
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", os.path.join(ROOT, "target", "sec-cand"),
                      PROBE, "-I", os.path.join(CAND_DIR, "include", "libxml2"),
                      "-L", CAND_DIR, "-Wl,-rpath", CAND_DIR, "-lxml2"])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        sys.exit(1)

    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = CAND_DIR + os.pathsep + env.get("LD_LIBRARY_PATH", "")
    ro, o_out, o_err = run([os.path.join(ROOT, "target", "sec-oracle")])
    # candidate run needs the candidate DSO on the loader path
    rc, c_out, c_err = run_with_env([os.path.join(ROOT, "target", "sec-cand")], env)

    verdict = "PASS" if (o_out == c_out and o_err == c_err and ro == rc) else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = os.path.join(RECEIPTS, f"security-limits-{ts}.json")
    with open(receipt, "w") as f:
        json.dump({
            "court": "SECURITY-LIMITS",
            "phase": "11.1-V",
            "timestamp": ts,
            "oracle": {"dso": ORACLE_SO, "argv": ["-I/usr/include/libxml2", "-lxml2"]},
            "candidate": {"dso": os.path.join(CAND_DIR, "liblibxml_rs.so"),
                          "argv": ["-I", os.path.join(CAND_DIR, "include", "libxml2"),
                                   "-L", CAND_DIR, "-Wl,-rpath", CAND_DIR, "-lxml2"]},
            "verdict": verdict,
            "oracle_stdout": b2s(o_out),
            "candidate_stdout": b2s(c_out),
            "oracle_stderr": b2s(o_err)[:2000],
            "candidate_stderr": b2s(c_err)[:2000],
            "exit_codes": {"oracle": ro, "candidate": rc},
        }, f, indent=1)
    print(f"receipt -> {receipt}")
    print(f"verdict={verdict} exit_oracle={ro} exit_candidate={rc}")
    if o_out != c_out:
        ol = b2s(o_out).splitlines()
        cl = b2s(c_out).splitlines()
        for i, (a, b) in enumerate(zip(ol, cl)):
            if a != b:
                print(f"first diff at line {i}:\n  oracle:    {a}\n  candidate: {b}")
                break
        else:
            print("line-count differs:", len(ol), "vs", len(cl))
    if o_err != c_err:
        print("STDERR DIFFERS (oracle/candidate):")
        print("  oracle:    ", b2s(o_err)[:300])
        print("  candidate: ", b2s(c_err)[:300])
    sys.exit(0 if verdict == "PASS" else 1)


def run_with_env(cmd, env):
    r = subprocess.run(cmd, capture_output=True, env=env)
    return r.returncode, r.stdout, r.stderr


if __name__ == "__main__":
    main()
