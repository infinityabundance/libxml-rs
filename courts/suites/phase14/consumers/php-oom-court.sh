#!/bin/bash
#
# php-oom-court.sh — reproducible Phase-14 PHP court runner (OOM-protected).
#
# Runs the SAME pinned PHP 8.5.10 six-extension suite against either the
# oracle (in-image /usr/local libxml2 2.15.3) or the candidate (host
# target/debug mounted at /candidate), each inside a memory/CPU-capped Docker
# container backed by a persistent volume, and writes a machine-readable JSON
# result + the full run log.
#
# Design principles (per phase-14 court contract):
#   * everything runs in a Docker-Hub-minimal image (libxml-rs/phase14-debian:1,
#     built from courts/suites/phase14/docker/Dockerfile.debian)
#   * the oracle is the canonical pinned-source build baked into the image
#   * candidate DSOs are mounted read-only from the host target/debug
#   * php-src and build state live in persistent volumes so a crash never
#     loses a completed build; only /out changes per run
#   * every run is OOM-capped (--memory/--memory-swap) and CPU-limited so it
#     reproduces deterministically on small hosts
#
# Usage:
#   phase14 php-oom-court.sh candidate      # build (if needed) + run candidate
#   phase14 php-oom-court.sh oracle         # build (if needed) + run oracle
#   PHASE14_PHP_TREE=<vol:path> ... overrides (see below)
#
# Env overrides:
#   CAND_PHP_VOL=phpC      candidate php-src volume   (default phpC)
#   ORA_PHP_VOL=phpO       oracle   php-src volume     (default phpO)
#   OUT_VOL=phpOut         results volume             (default phpOut)
#   CAND_PATH=/srcb        candidate volume mountpath (default /srcb)
#   ORA_PATH=/srco         oracle   volume mountpath  (default /srco)
#   IMG=libxml-rs/phase14-debian:1
#   REPO=/mnt/1tb_kingston/libxml-rs            (candidate DSO source)
#   TESTS="ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl"
#   MEM=6g CPUS=14
set -uo pipefail

IMG="${IMG:-libxml-rs/phase14-debian:1}"
REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
TESTS="${TESTS:-ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl}"
MEM="${MEM:-6g}"; CPUS="${CPUS:-14}"
CAND_PHP_VOL="${CAND_PHP_VOL:-phpC}"; ORA_PHP_VOL="${ORA_PHP_VOL:-phpO}"; OUT_VOL="${OUT_VOL:-phpOut}"
CAND_PATH="${CAND_PATH:-/srcb}"; ORA_PATH="${ORA_PATH:-/srco}"
MODE="${1:?usage: php-oom-court.sh <oracle|candidate>}"

for v in "$CAND_PHP_VOL" "$ORA_PHP_VOL" "$OUT_VOL"; do
  docker volume create "$v" >/dev/null 2>&1 || true
done
docker image inspect "$IMG" >/dev/null 2>&1 || { echo "image $IMG missing; build with Dockerfile.debian"; exit 2; }

case "$MODE" in
  oracle)     NAME="phporacle"; PHPB=1; VOL="$ORA_PHP_VOL"; MP="$ORA_PATH" ;;
  candidate)  NAME="phpbuild";  PHPB=1; VOL="$CAND_PHP_VOL"; MP="$CAND_PATH"
              CAND_MOUNT=" -v $REPO/target/debug:/candidate:ro" ;;
  *) echo "unknown mode $MODE"; exit 2 ;;
esac

# Persistent (memory-capped) runner on the mode's volume.
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" --memory="$MEM" --memory-swap="$MEM" --cpus="$CPUS" \
  -v "$VOL:$MP" -v "$OUT_VOL:/out" \
  -v "$REPO/courts/suites/phase14/consumers:/court/consumers:ro" ${CAND_MOUNT:-} \
  "$IMG" sleep infinity >/dev/null
echo "container $NAME started (mem=$MEM cpus=$CPUS)"

# Ensure php-src is present (extract the pinned tarball once).
docker exec "$NAME" bash -lc "test -f $MP/php-src/configure" || \
docker exec "$NAME" bash -lc "rm -rf $MP/php-src && mkdir -p $MP/php-src && tar xf /src/php-8.5.10.tar.gz -C $MP/php-src --strip-components=1" || exit 3

docker exec "$NAME" bash -lc "
  set -uo pipefail
  source /court/consumers/lib.sh $MODE
  cd $MP/php-src
  # configure once with the faithful 8.5 flags (--with-xsl, not the stale --enable-xsl)
  if [ ! -f config.status ]; then
    ./configure --prefix=/usr/local/php --disable-all --enable-cli \\
      --enable-dom --enable-simplexml --enable-xml --enable-xmlreader \\
      --enable-xmlwriter --with-xsl --with-libxml > /out/php-$MODE-configure.log 2>&1 || exit 4
  fi
  test -x sapi/cli/php || make -j$CPUS > /out/php-$MODE-make.log 2>&1 || exit 5
  find ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl \\
      -name '*.diff' -o -name '*.out' -o -name '*.exp' 2>/dev/null | xargs -r rm -f
  make test TESTS=\"$TESTS\" NO_INTERACTION=1 REPORT_EXIT_STATUS=1 > /out/php-$MODE-full.log 2>&1
  echo 'rc='\$?
"
echo "===== $MODE summary ====="
docker exec "$NAME" bash -lc "grep -aE 'Number of tests|Tests failed|Tests passed|Tests skipped' /out/php-$MODE-full.log | tail -4"
