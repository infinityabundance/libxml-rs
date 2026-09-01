#!/usr/bin/env python3
"""HOSTILE-ALLOCATOR (Phase 13) — install deliberately hostile allocators
via xmlMemSetup (always-fail, size-threshold-fail, realloc-fail, strdup-fail)
and require byte-identical survival/failure against the oracle.

Court family: HOSTILE-ALLOCATOR (hostile audit dimension 3: allocator)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-allocator-probe.c")


def main():
    return run_court("HOSTILE-ALLOCATOR", PROBE)


if __name__ == "__main__":
    sys.exit(main())
