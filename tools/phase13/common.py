#!/usr/bin/env python3
"""Shared Phase-13 hostile-audit runner plumbing.

Every Phase-13 attack court compiles one C probe against the system oracle
and against the candidate headers + DSO, runs both, and requires
byte-identical stdout/stderr/exit. Divergences are printed with the first
differing line. Receipts land in courts/receipts/phase-13/.
"""
import datetime
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CAND_DIR = os.path.join(ROOT, "target", "debug")
ORACLE_SO = "/usr/lib/libxml2.so.16"
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-13")
ORACLE_CFLAGS = ["-I", "/usr/include/libxml2", "-I", "/usr/include/libxslt",
                 "-I", "/usr/include/libexslt"]
CAND_CFLAGS = ["-I", os.path.join(CAND_DIR, "include", "libxml2"),
               "-I", os.path.join(CAND_DIR, "include")]
CAND_LDFLAGS = ["-L", CAND_DIR, "-Wl,-rpath", CAND_DIR]


def run(cmd, env=None):
    r = subprocess.run(cmd, capture_output=True, env=env)
    return r.returncode, r.stdout, r.stderr


def b2s(b):
    return b.decode("utf-8", "replace")


def run_court(name, probe, extra_oracle_libs=("-lxml2",),
              extra_cand_libs=("-lxml2",), phase="13"):
    """Build + run a probe against both sides and require byte-identity."""
    os.makedirs(RECEIPTS, exist_ok=True)
    tag = name.lower().replace(" ", "-")
    oracle_bin = os.path.join(CAND_DIR, f"{tag}-oracle")
    cand_bin = os.path.join(CAND_DIR, f"{tag}-cand")

    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", oracle_bin,
                      probe] + ORACLE_CFLAGS + list(extra_oracle_libs))
    if rc != 0:
        print(f"ORACLE COMPILE FAILED for {name}:\n" + b2s(err)[-2000:])
        return 1
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o", cand_bin,
                      probe] + CAND_CFLAGS + CAND_LDFLAGS + list(extra_cand_libs))
    if rc != 0:
        print(f"CANDIDATE COMPILE FAILED for {name}:\n" + b2s(err)[-2000:])
        return 1

    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = CAND_DIR + os.pathsep + env.get("LD_LIBRARY_PATH", "")
    ro, o_out, o_err = run([oracle_bin])
    rc, c_out, c_err = run([cand_bin], env)

    verdict = "PASS" if (o_out == c_out and o_err == c_err and ro == rc
                         and ro in (0, 1)) else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = os.path.join(RECEIPTS, f"{tag}-{ts}.json")
    with open(receipt, "w") as f:
        json.dump({
            "court": name,
            "phase": phase,
            "timestamp": ts,
            "probe": probe,
            "oracle": {"dso": ORACLE_SO,
                       "argv": ["gcc", "-std=c11", "-w"] + ORACLE_CFLAGS
                               + list(extra_oracle_libs)},
            "candidate": {"argv": ["gcc", "-std=c11", "-w"] + CAND_CFLAGS
                                  + CAND_LDFLAGS + list(extra_cand_libs)},
            "verdict": verdict,
            "exit_codes": {"oracle": ro, "candidate": rc},
            "exit_signal_clean": ro in (0, 1),
            "stdout_identical": o_out == c_out,
            "stderr_identical": o_err == c_err,
            "oracle_stdout": b2s(o_out),
            "candidate_stdout": b2s(c_out),
            "oracle_stderr": b2s(o_err)[:2000],
            "candidate_stderr": b2s(c_err)[:2000],
        }, f, indent=1)
    print(f"receipt -> {receipt}")
    print(f"verdict={verdict} exit_oracle={ro} exit_candidate={rc}")
    if o_out != c_out:
        ol = b2s(o_out).splitlines()
        cl = b2s(c_out).splitlines()
        shown = 0
        for i in range(max(len(ol), len(cl))):
            a = ol[i] if i < len(ol) else "<EOF>"
            b = cl[i] if i < len(cl) else "<EOF>"
            if a != b:
                print(f"  stdout diff line {i}:\n    oracle:    {a}\n    candidate: {b}")
                shown += 1
                if shown >= 10:
                    break
    if o_err != c_err:
        print("STDERR DIFFERS (first 300 chars):")
        print("  oracle:    ", b2s(o_err)[:300])
        print("  candidate: ", b2s(c_err)[:300])
    return 0 if verdict == "PASS" else 1


def main():
    """Subclasses override this via run_court calls."""
    return 0


if __name__ == "__main__":
    sys.exit(main())
