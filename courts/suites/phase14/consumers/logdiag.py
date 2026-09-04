#!/usr/bin/env python3
# logdiag.py — structural diagnostic for make-test logs (parse model mirror of
# php-court-failnames.py). Prints per-status totals, and for each FAIL pairing
# the raw log line numbers + the raw (pre-ANSI-strip) result lines, so we can
# see whether result lines really exist / how they are formatted.
import re
import sys

ANSI = re.compile(r"\x1b\[[0-9;]*m")
TEST = re.compile(r"^TEST\s+(\d+)/(\d+)\s+\[(.+?)\]\s*$")
RES = re.compile(r"^(PASS|FAIL|SKIP|XFAIL|BORK|WARN|LEAK|EXFAIL|XLEAK)\b(.*)$")

path = sys.argv[1]
raw_lines = open(path, "rb").read().decode("utf-8", "replace").splitlines()

cur = None
cur_lineno = None
counts = {}
fail_pairs = []
for i, line in enumerate(raw_lines, 1):
    plain = ANSI.sub("", line).strip("\r\n")
    m = TEST.match(plain)
    if m:
        cur = m.group(3).strip()
        cur_lineno = i
        continue
    r = RES.match(plain)
    if r and cur is not None:
        status = r.group(1)
        counts[status] = counts.get(status, 0) + 1
        if status in ("FAIL", "XFAIL", "LEAK"):
            fail_pairs.append((cur_lineno, i, status, cur, line))
        cur = None

print("file:", path)
print("total raw lines:", len(raw_lines))
print("status counts:", counts)
print("fail pairs:", len(fail_pairs))
for tline, rline, status, name, raw in fail_pairs:
    print("--- pair TEST@%d RESULT@%d %s" % (tline, rline, status))
    print("    test name:", name)
    print("    raw result line:", repr(raw))
