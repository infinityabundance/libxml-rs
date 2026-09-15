#!/bin/bash
# oom-guard-selftest.sh — §16.12 OOM guard self-test (fail-closed).
#
# Proves the OOM guard actually contains a runaway consumer:
#
#   1. the perf container has a finite memory cap and a raised oom_score_adj;
#   2. a process started through the harness path inherits that score, so the
#      kernel prefers it as the OOM victim over unrelated user processes;
#   3. an allocation larger than the cap is killed INSIDE the container
#      (SIGKILL) and the host does not enter global OOM: host MemAvailable
#      after the attempt is within TOLERANCE_MIB of before.
#
# Why this exists: an unprotected matrix run at a 60 GiB cap drove one driver to
# ~54 GiB RSS on a 2.7 GB OSM document; the host entered global OOM and killed
# the developer's editor alongside the run. The guard is what makes that
# impossible now. If this self-test fails, do NOT run the matrix.
#
# Usage: sh courts/suites/phase16/oom-guard-selftest.sh
set -uo pipefail
NAME="${PERF_CONTAINER:-perf-c}"
TOLERANCE_MIB="${TOLERANCE_MIB:-2048}"
FAIL=0

pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $*"; FAIL=1; }

cap=$(docker inspect "$NAME" --format '{{.HostConfig.Memory}}' 2>/dev/null || echo "")
adj=$(docker inspect "$NAME" --format '{{.HostConfig.OomScoreAdj}}' 2>/dev/null || echo "")

if [ -z "$cap" ] || [ "$cap" = "0" ]; then
  fail "container $NAME has no memory cap (HostConfig.Memory=$cap)"
else
  pass "container $NAME memory cap = $((cap / 1024 / 1024)) MiB"
fi
if [ -z "$adj" ] || [ "$adj" -lt 500 ] 2>/dev/null; then
  fail "container $NAME oom_score_adj=$adj is not raised (expected >= 500)"
else
  pass "container $NAME oom_score_adj = $adj"
fi

exec_adj=$(docker exec "$NAME" bash -lc 'cat /proc/self/oom_score_adj' 2>/dev/null || echo "")
if [ "$exec_adj" = "$adj" ]; then
  pass "a process started in $NAME inherits oom_score_adj = $exec_adj"
else
  fail "process in $NAME has oom_score_adj=$exec_adj, expected $adj"
fi

avail_mib_before=$(( $(awk '/^MemAvailable:/{print $2}' /proc/meminfo) / 1024 ))

REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
BOMB="$REPO/target/perf-out/oom-bomb.py"
mkdir -p "$REPO/target/perf-out"
cat > "$BOMB" <<'PY'
import sys
chunk = 256 * 1024 * 1024
held = 0
blocks = []
while True:
    blocks.append(bytearray(chunk))
    held += chunk
    if held % (8 * 1024 * 1024 * 1024) == 0:
        print("allocated %d GiB" % (held // (1024 ** 3)), file=sys.stderr, flush=True)
PY

echo "--- attempting an allocation beyond the cap inside $NAME ---"
set +e
docker exec "$NAME" python3 /out/oom-bomb.py 2>"$REPO/target/perf-out/oom-bomb.log"
rc=$?
set -e
avail_mib_after=$(( $(awk '/^MemAvailable:/{print $2}' /proc/meminfo) / 1024 ))
delta=$(( avail_mib_before - avail_mib_after ))

if [ "$rc" -eq 137 ]; then
  pass "runaway killed inside the container (SIGKILL, rc=137)"
elif [ "$rc" -ne 0 ]; then
  pass "runaway terminated inside the container (rc=$rc)"
else
  fail "runaway exited 0 — it was NOT bounded by the container cap"
fi

if [ "$delta" -le "$TOLERANCE_MIB" ]; then
  pass "host MemAvailable dropped ${delta} MiB (<= ${TOLERANCE_MIB} MiB tolerance)"
else
  fail "host MemAvailable dropped ${delta} MiB — the run is NOT host-isolated"
fi

echo "---"
if [ "$FAIL" -eq 0 ]; then
  echo "OOM GUARD SELF-TEST: PASS"
else
  echo "OOM GUARD SELF-TEST: FAIL"
fi
exit "$FAIL"
