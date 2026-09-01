#!/bin/bash
# lxml-run.sh — Phase 14 lxml court, in-container runner.
#
# Usage: lxml-run.sh <oracle|candidate>
# Builds the pinned lxml (git tag lxml-6.1.2) against the selected libxml2,
# runs the differential operation corpus + the lxml test suite (the repo's
# canonical `python3 test.py` unittest runner, Makefile TESTFLAGS=-p -vv
# reduced to -q for deterministic output), and writes:
#   /out/<mode>-build.log      build output
#   /out/<mode>-libver.txt     pkg-config versions actually resolved
#   /out/<mode>-ldd.txt        linked libxml2/libxslt DSOs of the extension
#   /out/<mode>-corpus.txt     differential corpus fingerprints
#   /out/<mode>-tests.log      unittest run of src/lxml/tests
#   /out/<mode>-result.json    machine-readable summary
set -uo pipefail
MODE="${1:?usage: lxml-run.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src/lxml

# ── build against the selected libxml2/libxslt ────────────────────────────
python3 setup.py build_ext --inplace --force > "/out/${MODE}-build.log" 2>&1
BUILD_RC=$?

pkg-config --modversion libxml-2.0 libxslt 2>/dev/null > "/out/${MODE}-libver.txt" || true
ldd src/lxml/etree*.so 2>/dev/null | grep -E "libxml2|libxslt|libexslt" \
    | sed 's/^[[:space:]]*//;s/=>.*//' > "/out/${MODE}-ldd.txt" || true

if [ "$BUILD_RC" -ne 0 ]; then
    echo "{\"consumer\":\"lxml\",\"mode\":\"$MODE\",\"build_ok\":false,\"build_tail\":\"$(tail -c 2000 /out/${MODE}-build.log | tr '\n' ' ' | sed 's/"/\\"/g')\"}" \
        > "/out/${MODE}-result.json"
    exit 0
fi

export PYTHONPATH=/src/lxml/src

# ── differential corpus ────────────────────────────────────────────────────
python3 /court/consumers/lxml-diffcorpus.py > "/out/${MODE}-corpus.txt" 2>&1
CORPUS_RC=$?

# ── lxml test suite (canonical runner: python test.py) ───────────────────
# TESTFLAGS=-p -vv in the Makefile; -u -v keeps the "Ran N tests" summary
# deterministic (progress dots would be identical but add noise).
python3 test.py -u -v > "/out/${MODE}-tests.log" 2>&1
TEST_RC=$?

python3 - "$MODE" "$CORPUS_RC" "$TEST_RC" <<'PYEOF' > "/out/${MODE}-result.json"
import json
import re
import sys

mode, corpus_rc, test_rc = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
log = open(f"/out/{mode}-tests.log", encoding="utf-8", errors="replace").read()
summary = None
m = re.search(r"Ran (\d+) tests? in", log)
ok = "OK" in log.splitlines()[-1] if log.splitlines() else False
m2 = re.search(r"FAILED \((failures=(\d+), errors=(\d+)|errors=(\d+)|failures=(\d+))\)", log)
summary = None
if m:
    summary = {"ran": int(m.group(1)), "ok": ok,
               "failures": int(m2.group(2) or m2.group(4) or 0) if m2 else 0,
               "errors": int(m2.group(3) or m2.group(5) or 0) if m2 else 0}
# failing test ids
fails = re.findall(r"^(?:FAIL|ERROR): ([\w.]+)", log, re.M)
print(json.dumps({
    "consumer": "lxml",
    "mode": mode,
    "build_ok": True,
    "corpus_rc": corpus_rc,
    "test_rc": test_rc,
    "test_summary": summary,
    "test_failed_ids": fails[:50],
    "corpus_tail": open(f"/out/{mode}-corpus.txt", encoding="utf-8", errors="replace").read()[-500:],
}))
PYEOF

echo "lxml-run ${MODE} done rc=${BUILD_RC}/${CORPUS_RC}/${TEST_RC}"
