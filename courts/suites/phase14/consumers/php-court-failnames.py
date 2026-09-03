#!/usr/bin/env python3
# php-court-failnames.py — extract the per-test FAIL set from a `make test`
# log and optionally diff two logs.
#
# Real log line format (make test, pty):
#   TEST <n>/<N> [<relpath>]\r<ESC>[1;3Xm<STATUS><ESC>[0m <desc> [<relpath>]
# So: match the leading "TEST n/N [path]" then take the first status token
# after the ANSI sequences on the SAME line.
#
# Usage:
#   php-court-failnames.py <log>              # sorted FAIL phpt paths + TOTAL
#   php-court-failnames.py --diff old new     # FIXED/NEW_ONLY vs the old log
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
LEAD = re.compile(r"^TEST\s+\d+/\d+\s+\[([^]]+\.phpt)\]")
STATUS = re.compile(r"^(PASS|FAIL|SKIP|XFAIL|BORK|WARN|LEAK|EXFAIL|XLEAK)\b")


def fail_set(path):
    fails = set()
    with open(path, "rb") as f:
        raw = f.read().decode("utf-8", "replace")
    for line in raw.split("\n"):
        # The pty log embeds a carriage return before the ANSI-colored status
        # token; splitlines() would treat that \r as a terminator and break
        # the single physical "TEST n/N [path]\r<STATUS> ..." line apart.
        l = ANSI.sub("", line).rstrip("\r")
        m = LEAD.match(l)
        if not m:
            continue
        # status token is the first word of the remainder
        rest = l[m.end():].lstrip()
        st = STATUS.match(rest)
        if st and st.group(1) in ("FAIL", "XFAIL", "LEAK"):
            fails.add(m.group(1))
    return fails


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--diff":
        old = fail_set(sys.argv[2])
        new = fail_set(sys.argv[3])
        fixed = sorted(old - new)
        fresh = sorted(new - old)
        print("OLD=%d NEW=%d FIXED=%d NEW_ONLY=%d" % (len(old), len(new), len(fixed), len(fresh)))
        for p in fixed:
            print("FIXED " + p)
        for p in fresh:
            print("NEW_ONLY " + p)
        return
    s = fail_set(sys.argv[1])
    for p in sorted(s):
        print(p)
    print("TOTAL %d" % len(s))


if __name__ == "__main__":
    main()
