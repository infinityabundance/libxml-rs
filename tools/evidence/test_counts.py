#!/usr/bin/env python3
"""Test-count evidence capture (11.1-W).

Runs the library test battery and records the canonical counts plus the
per-module breakdown to atlas/TEST_COUNTS.json, so the README headline counts
are generated from evidence instead of hand-typed. The 11.1-Z seal battery
runs the same commands and must reproduce these numbers.

Usage:
  test_counts.py          run `cargo test --lib` and write atlas/TEST_COUNTS.json
"""
import datetime
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATLAS = os.path.join(ROOT, "atlas")
OUT = os.path.join(ATLAS, "TEST_COUNTS.json")

# Module root (first two path components) -> README subsystem display name.
# Keyed on the module paths emitted by `cargo test --lib -- --list`.
MODULE_DISPLAY = {
    "abi::allocator": "ABI allocator",
    "abi::exports_xslt_avt": "ABI (xslt exports)",
    "compatibility::profiles": "Compatibility profiles",
    "exslt::common": "EXSLT common",
    "exslt::dates": "EXSLT dates",
    "exslt::dynamic": "EXSLT dynamic",
    "exslt::functions": "EXSLT functions",
    "exslt::math": "EXSLT math",
    "exslt::sets": "EXSLT sets",
    "exslt::strings": "EXSLT strings",
    "exslt::tests": "EXSLT registry",
    "xml::automata": "Automata",
    "xml::c14n": "C14N",
    "xml::catalog": "Catalog",
    "xml::chvalid": "Char validation",
    "xml::debug": "Debug",
    "xml::dictionary": "Dictionary",
    "xml::dtd": "DTD",
    "xml::encoding": "Encoding",
    "xml::entities": "Entities",
    "xml::errors": "Errors",
    "xml::globals": "Globals",
    "xml::hash": "Hash",
    "xml::html": "HTML",
    "xml::io": "I/O",
    "xml::list": "List",
    "xml::memory": "Memory",
    "xml::parser": "XML parser + SAX",
    "xml::reader": "XML Reader",
    "xml::regex": "Regex",
    "xml::relaxng": "RELAX NG",
    "xml::save": "Serialization",
    "xml::schemas": "XML Schema (XSD)",
    "xml::schematron": "Schematron",
    "xml::string": "String",
    "xml::threads": "Threads",
    "xml::tree": "Tree/ownership",
    "xml::uri": "URI",
    "xml::validation": "DTD validation",
    "xml::writer": "XML Writer",
    "xml::xinclude": "XInclude",
    "xml::xpath": "XPath 1.0",
    "xml::xpointer": "XPointer",
    "xslt::attributes": "XSLT misc (attrs)",
    "xslt::compiler": "XSLT compiler",
    "xslt::documents": "XSLT documents",
    "xslt::extensions": "XSLT extensions",
    "xslt::imports": "XSLT imports",
    "xslt::keys": "XSLT keys",
    "xslt::namespace_alias": "XSLT namespace alias",
    "xslt::numbering": "XSLT numbering",
    "xslt::parameters": "XSLT params",
    "xslt::patterns": "XSLT patterns",
    "xslt::security": "XSLT security",
    "xslt::serialization": "XSLT serialization",
    "xslt::sorting": "XSLT sorting",
    "xslt::stylesheet": "XSLT stylesheet",
    "xslt::transform": "XSLT transform",
    "xslt::variables": "XSLT variables/params",
    "xslt::whitespace": "XSLT whitespace",
}


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, timeout=3600)
    return r.returncode, (r.stdout or "") + (r.stderr or "")


def parse_test_result(out):
    m = re.search(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", out)
    if m:
        return {"status": "ok" if m.group(1) == "ok" else "FAILED",
                "passed": int(m.group(2)), "failed": int(m.group(3)),
                "ignored": int(m.group(4))}
    return None


def module_breakdown(list_out):
    """Count `cargo test --lib -- --list` lines per module root."""
    counts = {}
    for line in list_out.splitlines():
        line = line.strip()
        if not line.endswith(": test"):
            continue
        path = line[:-len(": test")]
        parts = path.split("::")
        key = "::".join(parts[:2])
        counts[key] = counts.get(key, 0) + 1
    return counts


def main():
    rc, out = run(["cargo", "test", "--lib"])
    lib = parse_test_result(out)
    if lib is None:
        print("could not parse `cargo test --lib` output:\n", out[-3000:])
        return 1
    rc2, list_out = run(["cargo", "test", "--lib", "--", "--list"])
    by_module = module_breakdown(list_out)
    if sum(by_module.values()) != lib["passed"] + lib["ignored"]:
        print(f"module breakdown {sum(by_module.values())} != executed "
              f"{lib['passed'] + lib['ignored']}")
        return 1

    subsystems = {}
    unmapped = []
    for key, n in sorted(by_module.items()):
        disp = MODULE_DISPLAY.get(key)
        if disp is None:
            unmapped.append(key)
            disp = key
        subsystems[disp] = subsystems.get(disp, 0) + n
    if unmapped:
        print("WARNING: unmapped test modules:", unmapped)

    doc = {
        "schema": "test-counts-1",
        "generator": "tools/evidence/test_counts.py",
        "generated": datetime.datetime.now(datetime.timezone.utc)
                     .strftime("%Y-%m-%dT%H:%M:%SZ"),
        "cargo_test_lib": lib,
        "subsystems": subsystems,
        "lib_verdict": "PASS" if lib["failed"] == 0 else "FAIL",
    }
    with open(OUT, "w") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"lib: {lib['passed']} passed, {lib['failed']} failed, "
          f"{lib['ignored']} ignored -> {OUT}")
    return 0 if lib["failed"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
