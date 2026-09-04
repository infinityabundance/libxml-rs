#!/usr/bin/env python3
# faildiff.py — robust FAIL-set extraction + diff for php make-test logs.
#
# The bundled php-court-failnames.py associates each FAIL line with the last
# seen "TEST n/m [..]" progress line.  Under parallel run-tests (-j4) the
# progress stream is redrawn with \r and periodic status lines such as
# "TEST 4/4 [4/4 concurrent test workers running]" clobber that association,
# collapsing many FAILs onto one junk name.  Here we instead take the path
# from the trailing "[ext/.../foo.phpt]" of each FAIL/XFAIL/LEAK result line,
# which is present verbatim regardless of progress redraws.
#
# Usage:
#   faildiff.py <log>            # status counts + sorted failing phpt paths
#   faildiff.py --diff old new   # FIXED / NEW_ONLY comparison
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
RES = re.compile(r"^(PASS|FAIL|SKIP|XFAIL|BORK|WARN|LEAK|EXFAIL|XLEAK)\b(.*)$")
PATH = re.compile(r"\[(ext/[^\]\s]+\.phpt)\]\s*$")
ALL_STATUS = ("PASS", "FAIL", "SKIP", "XFAIL", "BORK", "WARN", "LEAK", "EXFAIL", "XLEAK")


def parse(path):
    counts = {s: 0 for s in ALL_STATUS}
    fails = set()
    with open(path, "rb") as f:
        raw = f.read().decode("utf-8", "replace")
    for line in raw.splitlines():
        l = ANSI.sub("", line).strip("\r\n")
        r = RES.match(l)
        if not r:
            continue
        st = r.group(1)
        counts[st] += 1
        if st in ("FAIL", "XFAIL", "LEAK"):
            pm = PATH.search(l)
            if pm:
                fails.add(pm.group(1))
    return counts, fails


def report(path):
    counts, fails = parse(path)
    print("== %s" % path)
    print("counts: %s" % {k: v for k, v in counts.items() if v})
    print("failing phpt paths: %d" % len(fails))
    for p in sorted(fails):
        print("  " + p)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--diff":
        c1, old = parse(sys.argv[2])
        c2, new = parse(sys.argv[3])
        fixed = sorted(old - new)
        fresh = sorted(new - old)
        print("OLD_RAW=%d NEW_RAW=%d FIXED=%d NEW_ONLY=%d" %
              (len(old), len(new), len(fixed), len(fresh)))
        print("old-log counts: %s" % {k: v for k, v in c1.items() if v})
        print("new-log counts: %s" % {k: v for k, v in c2.items() if v})
        print("FIXED:")
        for p in fixed:
            print("  " + p)
        print("NEW_ONLY:")
        for p in fresh:
            print("  " + p)
        return
    for p in sys.argv[1:]:
        report(p)


if __name__ == "__main__":
    main()
