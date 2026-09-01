#!/usr/bin/env python3
"""HOSTILE-ABI (Phase 13) — attack the exported C ABI surface with NULLs,
invalid option bits, extreme/negative sizes and integer-boundary values.

Court family: HOSTILE-ABI (hostile audit dimension 1: ABI)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-abi-probe.c")


def main():
    return run_court("HOSTILE-ABI", PROBE)


if __name__ == "__main__":
    sys.exit(main())
