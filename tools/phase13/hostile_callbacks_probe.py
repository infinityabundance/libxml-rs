#!/usr/bin/env python3
"""HOSTILE-CALLBACKS (Phase 13) — exercise entity loaders, input/output I/O
callbacks and error handlers with adversarial behaviours (always-fail,
immediate-EOF, bytewise feeding, silent-write) and require byte-identical
outcomes against the oracle.

Court family: HOSTILE-CALLBACKS (hostile audit dimension 4: callbacks)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-callbacks-probe.c")


def main():
    return run_court("HOSTILE-CALLBACKS", PROBE)


if __name__ == "__main__":
    sys.exit(main())
