#!/bin/bash
# cand-six-gate.sh — run the full six-extension PHP suite against the CANDIDATE
# (host target/debug bind-mounted at /candidate) with an arbitrary log name.
#
# Usage: cand-six-gate.sh [LOGNAME]   (default xpe-six15.log; written to the
# phpOut docker volume, i.e. phpbuild-c:/out/<LOGNAME>)
#
# Iteration semantics: reuses the phpC volume tree (no reconfigure unless the
# candidate build-contract input hash changed; no rebuild when sapi/cli/php is
# fresh). Mirrors php-oom-court.sh container wiring exactly.
set -uo pipefail
REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
LOGNAME="${1:-xpe-six15.log}"
LOG="/out/${LOGNAME}"
NAME="phpbuild-c"
IMG="${IMG:-libxml-rs/phase14-debian:1}"
MEM="${MEM:-6g}"; CPUS="${CPUS:-14}"

docker volume create phpC >/dev/null 2>&1 || true
docker volume create phpOut >/dev/null 2>&1 || true
if ! docker ps --filter "name=^/${NAME}$" --format '{{.Names}}' | grep -qx "$NAME"; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  docker run -d --name "$NAME" --memory="$MEM" --memory-swap="$MEM" --cpus="$CPUS" \
    -v phpC:/srcb -v phpOut:/out \
    -v "$REPO/courts/suites/phase14/consumers:/court/consumers:ro" \
    -v "$REPO/target/debug:/candidate:ro" \
    "$IMG" sleep infinity >/dev/null
  echo "container $NAME (re)created"
fi

docker exec -e CPUS="$CPUS" "$NAME" bash -lc "
  set -uo pipefail
  source /court/consumers/lib.sh candidate
  source /court/consumers/php-court-spec.sh
  export MODE=candidate LOG='$LOG' CPUS=$CPUS
  cd /srcb
  /court/consumers/php-court-stage.sh candidate || exit \$?
"
echo "gate done; log: phpbuild-c:$LOG"
