#!/bin/bash
# Extract --FILE-- body from a phpt and run under a given php binary.
# usage: runphpt.sh <oracle|candidate> <phpt>
MODE="$1"; F="$2"
python3 - "$F" <<'PY'
import re, sys
p = sys.argv[1]
t = open(p).read()
# file section may end at --EXPECT/--EXPECTF/--EXPECTREGEX/--CREDITS/--CLEAN--
m = re.search(r'--FILE--\s*(.*?)(?=\n--(EXPECT|EXPECTF|EXPECTREGEX|CREDITS|CLEAN|SKIPIF|INI|ENV|ARGS))', t, re.S)
print(m.group(1), end='')
PY
