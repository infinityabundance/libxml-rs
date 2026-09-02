#!/bin/bash
#
# php-oom-court.sh — Phase-14.3 authoritative PHP court (fail-closed).
#
# Runs the pinned pristine PHP 8.5.10 six-extension suite
# (ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl)
# against EITHER the oracle (baked /usr/local libxml2 2.15.3 + libxslt 1.1.45)
# or the libxml-rs candidate (host target/debug mounted at /candidate),
# inside a memory/CPU-capped Docker container on a persistent volume that makes
# a crash never lose a completed build.
#
# Mode of execution (Phase 14.3-B):
#   PHASE14_PHP_MODE=iterate    reuse an intact, build-contract-valid PHP tree
#                               (fast iteration; only runtime-semantic work)
#   PHASE14_PHP_MODE=cleanseal  (default for seals) wipe php-src / configure /
#                               make / test from scratch. Mandatory for FINAL
#                               Phase-14.3 evidence.
#
# Fail-closed (Phase 14.3-A).  After the run a Python interpreter reconciles:
#     *.diff  file set  == log FAIL recounts  ==  global "Tests failed"
# and refuses (verdict=fail) on any unexplained/accounting violation. Emits:
#   /out/php-<mode>-result.json   machine-readable authoritative result
#   /out/php-<mode>-result.md     textual summary (agrees with the JSON)
#   /out/php-<mode>-full.log      the raw `make test` stream
# The JSON and .md are written from the SAME computed object so they can never
# disagree by construction.
#
# Usage:
#   php-oom-court.sh <oracle|candidate>
#
# Env:
#   PHASE14_PHP_MODE   iterate|cleanseal   (default iterate)
#   PHASE14_WRITE_RECEIPT  1               write a commit-ready receipt under
#                                          $REPO/courts/receipts/phase-14/
#   CAND_PHP_VOL / ORA_PHP_VOL / OUT_VOL   (phpC / phpO / phpOut)
#   IMG=libxml-rs/phase14-debian:1   REPO=<repo path>
#   MEM=6g CPUS=14
set -uo pipefail

IMG="${IMG:-libxml-rs/phase14-debian:1}"
REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
TESTS_SPEC="${TESTS:-ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl}"
MEM="${MEM:-6g}"; CPUS="${CPUS:-14}"
CAND_PHP_VOL="${CAND_PHP_VOL:-phpC}"; ORA_PHP_VOL="${ORA_PHP_VOL:-phpO}"; OUT_VOL="${OUT_VOL:-phpOut}"
CAND_PATH="${CAND_PATH:-/srcb}"; ORA_PATH="${ORA_PATH:-/srco}"
MODE="${1:?usage: php-oom-court.sh <oracle|candidate>}"
PMODE="${PHASE14_PHP_MODE:-iterate}"

case "$PMODE" in iterate|cleanseal) ;; *) echo "PHASE14_PHP_MODE must be iterate|cleanseal"; exit 2 ;; esac

for v in "$CAND_PHP_VOL" "$ORA_PHP_VOL" "$OUT_VOL"; do
  docker volume create "$v" >/dev/null 2>&1 || true
done
docker image inspect "$IMG" >/dev/null 2>&1 || { echo "image $IMG missing; build with Dockerfile.debian"; exit 2; }

case "$MODE" in
  oracle)    NAME="phporacle-c"; PHPB=1; VOL="$ORA_PHP_VOL"; MP="$ORA_PATH"; MOUNT="" ;;
  candidate) NAME="phpbuild-c";  PHPB=1; VOL="$CAND_PHP_VOL"; MP="$CAND_PATH"
             MOUNT=" -v $REPO/target/debug:/candidate:ro" ;;
  *) echo "unknown mode $MODE"; exit 2 ;;
esac
# disambiguate running containers so we never operate on a stale instance
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --memory="$MEM" --memory-swap="$MEM" --cpus="$CPUS" \
  -v "$VOL:$MP" -v "$OUT_VOL:/out" \
  -v "$REPO/courts/suites/phase14/consumers:/court/consumers:ro" $MOUNT \
  "$IMG" sleep infinity >/dev/null
echo "container $NAME started (mem=$MEM cpus=$CPUS mode=$PMODE mode-vol=$VOL)"

# CLEAN SEAL: wipe prior php-src + build + config state.
if [ "$PMODE" = "cleanseal" ]; then
  docker exec "$NAME" bash -lc "rm -rf $MP/php-src" || true
fi

# Build+test stage (extraction / configure / make / make test).
if [ "$PMODE" = "cleanseal" ]; then
  docker exec -e CPUS="$CPUS" "$NAME" bash -lc "
    set -uo pipefail
    source /court/consumers/lib.sh $MODE
    source /court/consumers/php-court-spec.sh
    export MODE=$MODE LOG=/out/php-$MODE-full.log \
           CFG_LOG=/out/php-$MODE-configure.log MAKE_LOG=/out/php-$MODE-make.log \
           CPUS=$CPUS FORCE_CLEAN=1 FORCE_CONF=1
    cd $MP
    /court/consumers/php-court-stage.sh $MODE || exit \$?
  "
  RUN_RC=$?
else
  docker exec -e CPUS="$CPUS" "$NAME" bash -lc "
    set -uo pipefail
    source /court/consumers/lib.sh $MODE
    source /court/consumers/php-court-spec.sh
    export MODE=$MODE LOG=/out/php-$MODE-full.log \
           CFG_LOG=/out/php-$MODE-configure.log MAKE_LOG=/out/php-$MODE-make.log \
           CPUS=$CPUS
    cd $MP
    /court/consumers/php-court-stage.sh $MODE || exit \$?
  "
  RUN_RC=$?
fi

# Parse + reconcile into authoritative JSON/MD inside the container (where the
# php-src tree and its *.diff artifacts are visible).
docker exec -e MODE="$MODE" -e PHP_TREE="$MP/php-src" \
  -e LOG="/out/php-$MODE-full.log" -e COURT_OUT="/out/php-$MODE-result" \
  -e EXT_DIRS="ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl" \
  -e PHP_VERSION="8.5.10" -e PHP_SHA256="f5c0ac99b85b3d677de475c2e4f509f9b4f54663f3ee5a84d6d9481a521d4100" \
  "$NAME" python3 /court/consumers/php-court-result.py > "/tmp/${MODE}-court.json" 2> "/tmp/${MODE}-court.err"
RES_RC=$?

echo "===== $MODE verdict ====="
if [ "$RES_RC" -ne 0 ]; then echo "result writer failed (rc=$RES_RC)"; cat "/tmp/${MODE}-court.err" | tail -20; fi
docker cp "/tmp/${MODE}-court.json" "$NAME":/out/php-${MODE}-result-inline.json >/dev/null 2>&1 || true
exit $RES_RC
