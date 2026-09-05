#!/usr/bin/env python3
"""DSO-STATE-COHERENCE-001 — cross-DSO shared-state court (11.1-Z.2).

Compiles courts/suites/data-abi/dso-state-coherence-probe.c twice:

  1. oracle  — against the system libxslt + libxml2 (-lxslt -lxml2)
  2. candidate — against the candidate three-DSO facades
               (-L target/debug -lxslt -lxml2, runpath target/debug)

for every observation the candidate must match the oracle: the allocator,
node register/deregister and external-entity-loader hooks installed through
the libxml2 DSO must be observed by the libxslt-DSO compile and transform
phases, the keepBlanks default must govern fresh-context parses inside the
transform, and the deterministic result sizes must agree.

HISTORY (R-000177): until Phase 14.30 the candidate's whole-archive facade
libxslt/libexslt carried private copies of the libxml2 core (its statics,
TLS cells, allocator slots and loader registration), so hooks installed
through the core DSO were invisible to facade-side work — the court PINNED
that documented divergence. The R-000177 fix bridged the process-visible
state (dlsym'd core accessors for the per-thread cells and the allocator /
entity-loader registrations) so facade copies observe consumer registrations
exactly like upstream's single shared libxml2 instance. The court now
asserts full PARITY with the oracle: any future regression to partitioned
state fails it.

Receipts are written to courts/receipts/phase-11/.

Usage:
    python3 tools/abi/dso_state_coherence_probe.py
"""

import datetime
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROBE = os.path.join(ROOT, "courts", "suites", "data-abi", "dso-state-coherence-probe.c")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")

OBS_KEYS = ("loader_observed_main_parse", "p1_allocator_observed",
            "p1_reg_observed", "p1_dereg_observed", "p2_allocator_observed",
            "p2_reg_observed", "p2_dereg_observed", "p2_loader_observed",
            "p2_result_size", "p2_result_has_entity")


def run(cmd, cwd=None):
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return r.returncode, r.stdout, r.stderr


def parse_output(output):
    obs = {}
    for line in output.splitlines():
        if "=" in line and line.split("=")[0] in OBS_KEYS:
            k, v = line.split("=", 1)
            try:
                obs[k] = int(v.strip())
            except ValueError:
                obs[k] = v.strip()
    return obs


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    work = os.path.join(ROOT, "target")

    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(work, "dsc-oracle"),
                      PROBE, "-I", "/usr/include/libxml2", "-I", "/usr/include",
                      "-lxslt", "-lxml2"])
    if rc != 0:
        print("ORACLE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, o_out, err = run([os.path.join(work, "dsc-oracle")], cwd=work)
    if rc != 0:
        print("ORACLE RUN FAILED:\n" + err[-2000:])
        return 1
    o_obs = parse_output(o_out)
    print("--- oracle observations:")
    for k in OBS_KEYS:
        print(f"    {k}={o_obs.get(k)}")
    # Oracle sanity: hooks must be observed where the shared instance
    # guarantees it (compile allocates but registers no nodes in upstream;
    # the transform creates AND frees nodes and loads document()).
    for k in ("p1_allocator_observed", "p2_allocator_observed",
              "p2_reg_observed", "p2_dereg_observed", "p2_loader_observed"):
        if not o_obs.get(k, 0):
            print(f"ORACLE UNEXPECTED: {k} not observed:\n{o_out}")
            return 1

    # ---- candidate build (candidate headers + facades) --------------------
    rc, _, err = run(["gcc", "-std=c11", "-o", os.path.join(work, "dsc-cand"),
                      PROBE, "-I", os.path.join(ROOT, "include"),
                      "-L", os.path.join(ROOT, "target", "debug"),
                      "-lxslt", "-lxml2",
                      "-Wl,-rpath," + os.path.join(ROOT, "target", "debug")])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + err[-2000:])
        return 1
    rc, c_out, err = run([os.path.join(work, "dsc-cand")], cwd=work)
    if rc != 0:
        print("CANDIDATE RUN FAILED:\n" + err[-2000:])
        return 1
    c_obs = parse_output(c_out)
    print("--- candidate observations:")
    for k in OBS_KEYS:
        print(f"    {k}={c_obs.get(k)}")

    # FULL PARITY (R-000177 FIXED, Phase 14.30): every observation must match
    # the oracle. The candidate's three-DSO construction now bridges the
    # process-visible state (core cells/registrations via dlsym'd accessors),
    # so hooks installed through the libxml2 DSO are observed by the libxslt
    # DSO exactly as with upstream's single shared instance.
    parity_ok = all(c_obs.get(k) == o_obs.get(k) for k in OBS_KEYS)
    mismatches = [k for k in OBS_KEYS if c_obs.get(k) != o_obs.get(k)]

    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "DSO-STATE-COHERENCE",
        "phase": "11.1-Z.2",
        "timestamp": ts,
        "schema": "dso-state-coherence-2",
        "probe": os.path.relpath(PROBE, ROOT),
        "mode": "full-parity (R-000177 bridged)",
        "oracle_observations": o_obs,
        "candidate_observations": c_obs,
        "parity_ok": parity_ok,
        "mismatches": mismatches,
        "candidate_output": c_out,
        "verdict": "PASS" if parity_ok else "FAIL",
    }
    rp = os.path.join(RECEIPTS, f"dso-state-coherence-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"verdict={'PASS' if parity_ok else 'FAIL'} mismatches={mismatches}")
    return 0 if parity_ok else 1


if __name__ == "__main__":
    sys.exit(main())
