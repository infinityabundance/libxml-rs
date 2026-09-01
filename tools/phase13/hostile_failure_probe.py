#!/usr/bin/env python3
"""HOSTILE-FAILURE (Phase 13) — drive the defined failure paths (depth
limits, entity loops, XPath/save/DTD/regexp/reader failures, amplification
guard) and require byte-identical failure behaviour against the oracle.

Court family: HOSTILE-FAILURE (hostile audit dimension 5: failure paths)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-failure-probe.c")


def main():
    return run_court("HOSTILE-FAILURE", PROBE)


if __name__ == "__main__":
    sys.exit(main())
