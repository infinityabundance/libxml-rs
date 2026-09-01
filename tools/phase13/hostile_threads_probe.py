#!/usr/bin/env python3
"""HOSTILE-THREADS (Phase 13) — attack threading/global-state semantics:
concurrent hostile parses, per-thread last-error TLS isolation, and
concurrent read-only global access. Requires byte-identical output.

Court family: HOSTILE-THREADS (hostile audit dimension 6: threading)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, run_court  # noqa: E402

PROBE = os.path.join(ROOT, "courts", "suites", "phase13", "hostile-threads-probe.c")


def main():
    # Needs pthreads.
    return run_court("HOSTILE-THREADS", PROBE,
                     extra_oracle_libs=("-lxml2", "-lpthread"),
                     extra_cand_libs=("-lxml2", "-lpthread"))


if __name__ == "__main__":
    sys.exit(main())
