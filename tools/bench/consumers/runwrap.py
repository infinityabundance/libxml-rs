#!/usr/bin/env python3
"""runwrap.py — run a consumer driver and report wall time + max RSS.

The perf image has no /usr/bin/time. This wrapper runs the driver as a child,
passes its stdout (the driver's JSON) through unchanged, inherits stderr, and
appends a machine-readable metrics line to stderr:

    TIME_S <seconds> RSS_KB <kilobytes>

Usage: python3 runwrap.py -- <driver> [args...]
"""

from __future__ import annotations

import resource
import subprocess
import sys
import time


def main() -> int:
    if "--" not in sys.argv:
        print("usage: runwrap.py -- cmd ...", file=sys.stderr)
        return 2
    argv = sys.argv[sys.argv.index("--") + 1:]
    t0 = time.perf_counter()
    p = subprocess.run(argv, stdout=subprocess.PIPE)
    dt = time.perf_counter() - t0
    sys.stdout.write(p.stdout.decode("utf-8", "replace"))
    ru = resource.getrusage(resource.RUSAGE_CHILDREN)
    sys.stderr.write("TIME_S %.6f RSS_KB %d\n" % (dt, ru.ru_maxrss))
    return p.returncode


if __name__ == "__main__":
    sys.exit(main())
