#!/usr/bin/env python3
"""DYNSYM-SURFACE-001 (Phase 12) — positive/negative dynamic-symbol court.

The disposition ledger (atlas/EXPORT_SURFACE_DISPOSITION.json) records the
shipped contract: everything except INTERNAL_LEAK is a deliberate dynamic
export; every INTERNAL_LEAK must be HIDDEN from the shared DSOs.

This court dlopens each shipped DSO and:

  positive — every shipped symbol (CURRENT_ORACLE_EXPORT,
             CUSTODIAN_EXTENSION, HISTORICAL_COMPAT_EXPORT) must resolve
             via dlsym on the owning DSO;
  negative — every INTERNAL_LEAK symbol must NOT be a defined dynamic
             export of the DSO it was observed on (the exact export maps hid
             it). The hidden check is the DSO's OWN dynsym (`nm -D
             --defined-only`): a handle-scoped dlsym would also resolve
             symbols the DSO merely inherits from its DT_NEEDED chain
             (libgcc_s compiler-rt builtins like __absvdi2, libc helpers),
             which are not leaks of the candidate implementation.

The negative test is the Phase-12 guarantee that "nothing accidentally
escaped from the Rust implementation".

Usage:
    python3 tools/phase12/dlsym_surface_court.py
"""
import ctypes
import datetime
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LIBDIR = os.path.join(ROOT, "target", "debug", "lib")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-12")

# shipped DSO path per project
DSOS = {
    "libxml2": os.path.join(LIBDIR, "libxml2.so.16.1.3"),
    "libxslt": os.path.join(LIBDIR, "libxslt.so.1.1.45"),
    "libexslt": os.path.join(LIBDIR, "libexslt.so.0.8.25"),
}
SONAMES = {"libxml2": "libxml2.so.16", "libxslt": "libxslt.so.1",
           "libexslt": "libexslt.so.0"}


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    ledger = json.load(open(os.path.join(ROOT, "atlas",
                                         "EXPORT_SURFACE_DISPOSITION.json")))
    cases = []
    pass_n = fail_n = 0

    def record(name, ok, detail=""):
        nonlocal pass_n, fail_n
        cases.append({"item": name, "status": "PASS" if ok else "FAIL",
                      "detail": detail})
        if ok:
            pass_n += 1
        else:
            fail_n += 1

    for project, so in DSOS.items():
        if not os.path.exists(so):
            record(f"{project}:dso-present", False, f"missing {so}")
            continue
        record(f"{project}:dso-present", True)
        try:
            h = ctypes.CDLL(so, mode=ctypes.RTLD_LOCAL)
        except OSError as e:
            record(f"{project}:dlopen", False, str(e))
            continue
        record(f"{project}:dlopen", True)

        syms = ledger["projects"][project]["symbols"]
        shipped = [n for n, r in syms.items()
                   if r["disposition"] != "INTERNAL_LEAK"]
        leaks = [n for n, r in syms.items()
                 if r["disposition"] == "INTERNAL_LEAK"]

        # positive: every shipped symbol resolves on its owning DSO
        missing = []
        for n in sorted(shipped):
            try:
                getattr(h, n)
            except AttributeError:
                missing.append(n)
        record(f"{project}:dlsym-positive ({len(shipped)} shipped)",
               not missing, "; ".join(missing[:8]))

        # negative: every INTERNAL_LEAK must not be a defined dynamic export
        # of THIS DSO (its own dynsym — NOT a handle dlsym, which also
        # resolves symbols inherited from the DT_NEEDED chain: libgcc_s
        # compiler-rt builtins, libc helpers)
        out = subprocess.run(["nm", "-D", "--defined-only", so],
                             capture_output=True, text=True).stdout
        defined = set()
        for line in out.splitlines():
            p = line.split()
            if len(p) < 3:
                continue
            name = p[2].split("@@")[0].split("@")[0]
            if p[1] == "A" or name in ("", "_edata", "_end", "__bss_start",
                                       "_init", "_fini"):
                continue
            defined.add(name)
        visible = sorted(n for n in leaks if n in defined)
        record(f"{project}:dlsym-negative ({len(leaks)} leaks hidden)",
               not visible, "; ".join(visible[:8]))

    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "DYNSYM-SURFACE",
        "phase": "12",
        "schema": "dynsym-surface-1",
        "timestamp": ts,
        "ledger": "atlas/EXPORT_SURFACE_DISPOSITION.json",
        "cases": cases,
        "summary": {"passed": pass_n, "failed": fail_n},
        "verdict": "PASS" if fail_n == 0 else "FAIL",
    }
    rp = os.path.join(RECEIPTS, f"dynsym-surface-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"passed={pass_n} failed={fail_n} "
          f"verdict={'PASS' if fail_n == 0 else 'FAIL'}")
    return 0 if fail_n == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
