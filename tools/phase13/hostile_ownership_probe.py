#!/usr/bin/env python3
"""HOSTILE-OWNERSHIP (Phase 13) — attack tree/document/buffer ownership
semantics: unattached nodes, unlink/re-add cycles, deep copies, sibling
insertion boundaries, node-registration hooks and lifecycle handoffs.

Court family: HOSTILE-OWNERSHIP (hostile audit dimension 2: ownership)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-ownership-probe.c")


def main():
    return run_court("HOSTILE-OWNERSHIP", PROBE)


if __name__ == "__main__":
    sys.exit(main())
