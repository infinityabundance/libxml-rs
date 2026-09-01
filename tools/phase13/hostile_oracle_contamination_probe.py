#!/usr/bin/env python3
"""HOSTILE-ORACLE-CONTAMINATION (Phase 13) — attack court, dimension 7:
the candidate must not secretly depend on the SYSTEM libxml2/libxslt/libexslt.

A hostile auditor substitutes the candidate DSOs and asks three questions:

  1. DYNAMIC-LOAD HYGIENE (readelf -d):
     - the core libxml2.so.16 must NOT list libxml2/libxslt/libexslt in
       DT_NEEDED (it is self-contained; only libc/libm/libgcc_s/ld);
     - the libxslt.so.1 facade must NEED the candidate libxml2.so.16 (and
       nothing xml-family beyond it); the libexslt.so.0 facade must NEED
       the candidate libxslt.so.1 + libxml2.so.16.
  2. UNDEFINED-SYMBOL HYGIENE (nm -D --undefined-only):
     - no unresolved `xml*`/`xslt*`/`exslt*` symbols in any candidate DSO
       (the loader could only satisfy them from the oracle DSOs).
  3. RUNTIME RESOLUTION (probe): linked xmlReadMemory/xsltApplyStylesheet/
     exsltRegisterAll and dlopen("libxml2.so.16|libxslt.so.1|libexslt.so.0")
     must resolve into the candidate directory (never /usr/lib), and the
     loaded parser must carry the candidate version marker.

The property is candidate-side (the oracle has no "contamination" risk), so
the verdict is assertion-based, not byte-identical.

Receipts: courts/receipts/phase-13/hostile-oracle-contamination-<ts>.json

Usage:
    python3 tools/phase13/hostile_oracle_contamination_probe.py
"""
import datetime
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, CAND_DIR, CAND_CFLAGS, CAND_LDFLAGS, b2s, run  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13",
                     "hostile-oracle-contamination-probe.c")
LIBDIR = os.path.join(CAND_DIR, "lib")
CORE = os.path.join(LIBDIR, "libxml2.so.16")
XSLT = os.path.join(LIBDIR, "libxslt.so.1.1.45")
EXSLT = os.path.join(LIBDIR, "libexslt.so.0.8.25")
VERSION_MARKER = "21503-GITv2.15.3"

XML_NAME = re.compile(r"\b(xml|xslt|exslt)[A-Za-z0-9_]*$")


def needed_of(so):
    out = subprocess.run(["readelf", "-d", so], capture_output=True,
                         text=True).stdout
    return [ln.split("[")[-1].rstrip("]").strip()
            for ln in out.splitlines() if "NEEDED" in ln]


def undefined_of(so):
    out = subprocess.run(["nm", "-D", "--undefined-only", so],
                         capture_output=True, text=True).stdout
    return [ln.split()[-1] for ln in out.splitlines() if ln.split()]


def main():
    os.makedirs(os.path.join(ROOT, "courts", "receipts", "phase-13"),
                exist_ok=True)
    findings = []
    checks = []

    def check(name, ok, detail):
        checks.append({"check": name, "ok": bool(ok), "detail": detail})
        if not ok:
            findings.append({"check": name, "detail": detail})

    # ── 1. DT_NEEDED hygiene ────────────────────────────────────────────────
    core_need = needed_of(CORE)
    xslt_need = needed_of(XSLT)
    exslt_need = needed_of(EXSLT)
    xml_family = {"libxml2.so.16", "libxslt.so.1", "libexslt.so.0"}
    check("core-no-xml-family-NEEDED",
          not (set(core_need) & xml_family),
          f"core NEEDED={core_need}")
    check("xslt-facade-NEEDs-candidate-core",
          "libxml2.so.16" in xslt_need
          and not (set(xslt_need) & {"libxslt.so.1", "libexslt.so.0"}),
          f"libxslt NEEDED={xslt_need}")
    check("exslt-facade-NEEDs-candidate-xslt+core",
          "libxslt.so.1" in exslt_need and "libxml2.so.16" in exslt_need
          and not (set(exslt_need) & {"libexslt.so.0"}),
          f"libexslt NEEDED={exslt_need}")

    # ── 2. undefined-symbol hygiene ─────────────────────────────────────────
    for label, so in (("core", CORE), ("libxslt", XSLT), ("libexslt", EXSLT)):
        undef = [u for u in undefined_of(so) if XML_NAME.search(u)]
        check(f"{label}-no-undefined-xml-family-symbols", not undef,
              f"undefined xml-family: {undef}")

    # ── 3. runtime resolution ───────────────────────────────────────────────
    rc, _, err = run(["gcc", "-std=c11", "-w", "-o",
                      os.path.join(CAND_DIR, "hostile-contam-cand"),
                      PROBE] + CAND_CFLAGS + CAND_LDFLAGS
                     + ["-lxml2", "-lxslt", "-lexslt", "-ldl"])
    if rc != 0:
        print("CANDIDATE COMPILE FAILED:\n" + b2s(err)[-2000:])
        return 1
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = LIBDIR + os.pathsep + CAND_DIR + os.pathsep \
        + env.get("LD_LIBRARY_PATH", "")
    rc, out, err = run([os.path.join(CAND_DIR, "hostile-contam-cand")], env)
    out = b2s(out)
    check("runtime-exit-clean", rc in (0, 1), f"exit={rc}")

    ok_all = out.count("=ok") == 3
    check("dlopen-all-three-SONAMEs", ok_all,
          [ln for ln in out.splitlines() if ln.startswith("dlopen")])

    resolved = {}
    for ln in out.splitlines():
        m = re.match(r"(xmlReadMemory|xsltApplyStylesheet|exsltRegisterAll) "
                     r"from: (.+)$", ln)
        if m:
            resolved[m.group(1)] = m.group(2)
    for sym, path in resolved.items():
        check(f"resolved-{sym}-inside-candidate",
              "/usr/" not in path and (LIBDIR in path or CAND_DIR in path),
              f"{sym} -> {path}")
    check("all-symbols-resolved", len(resolved) == 3,
          f"resolved={sorted(resolved)}")

    ver = [ln.split(": ", 1)[1] for ln in out.splitlines()
           if ln.startswith("xmlParserVersion: ")]
    check("candidate-version-marker", ver == [VERSION_MARKER],
          f"xmlParserVersion={ver}")

    verdict = "PASS" if not findings else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime(
        "%Y%m%dT%H%M%SZ")
    receipt = os.path.join(ROOT, "courts", "receipts", "phase-13",
                           f"hostile-oracle-contamination-{ts}.json")
    with open(receipt, "w") as f:
        json.dump({
            "court": "HOSTILE-ORACLE-CONTAMINATION",
            "phase": "13",
            "timestamp": ts,
            "probe": PROBE,
            "verdict": verdict,
            "checks": checks,
            "findings": findings,
            "runtime": {"exit": rc, "stdout": out, "stderr": b2s(err)[:1000]},
        }, f, indent=1)
    print(f"receipt -> {receipt}")
    print(f"verdict={verdict} checks={len(checks)} findings={len(findings)}")
    for c in checks:
        if not c["ok"]:
            print(f"  FAIL {c['check']}: {c['detail']}")
    for ln in out.splitlines():
        print("  " + ln)
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
