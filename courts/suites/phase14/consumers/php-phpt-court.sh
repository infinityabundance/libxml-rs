#!/bin/bash
# php-phpt-court.sh — Phase-14.3: run one (or a few) PHPTs against oracle / candidate.
#
# Usage (from courts/suites/phase14):
#   LANG=C ./consumers/php-phpt-court.sh <oracle|candidate> <test...>
# (paths relative to an extracted php tree, e.g.
#   ext/dom/tests/DOMDocument_loadXML_variation2.phpt
# )
#
# Preserves the REAL PHP harness semantics (make test TESTS=...) and captures the
# generated .exp/.out/.diff/.log*. Each test must exist in BOTH sides' php-src
# (identical by construction), so a side is selected the same way everywhere:
#   oracle    -> /srco php tree (baked oracle libxml in /usr/local)
#   candidate -> /srcb php tree (host target/debug mounted at /candidate)
#
# On success prints per-test PASS/FAIL + writes, on failure the .diff bodies.
set -uo pipefail
_mode="${1:?usage: php-phpt-court.sh <oracle|candidate> <phpt ...>}"; shift
[ $# -ge 1 ] || { echo "no phpt given"; exit 2; }

IMG="${IMG:-libxml-rs/phase14-debian:1}"
case "$_mode" in
  oracle)     ctr=phporacle-c ; mp=/srco ; extra="" ;;
  candidate)  ctr=phpbuild-c  ; mp=/srcb ; extra=" -v $PWD/../../../target/debug:/candidate:ro" ;;
  *) echo "unknown mode $_mode"; exit 2 ;;
esac

# Ensure a persistent container is up with consumers mounted.
docker rm -f "$ctr" >/dev/null 2>&1 || true
docker run -d --name "$ctr" --memory=6g --memory-swap=6g --cpus=14 \
  -v "${_mode}:${mp}" -v phpOut:/out \
  -v "$PWD/consumers:/court/consumers:ro" $extra \
  "$IMG" sleep infinity >/dev/null

tests="$*"
docker exec -e CPUS=14 -e PHP_TESTS_LIST="$tests" -e LOG=/out/php-${_mode}-phpt.log "$ctr" \
  bash -lc "
    set -uo pipefail
    cd $mp
    source /court/consumers/lib.sh $_mode
    source /court/consumers/php-court-spec.sh
    export MODE=$_mode CPUS=14 PHP_SRC='$mp/php-src'
    /court/consumers/php-court-stage.sh $_mode
    rc=\$?
    echo 'stage rc='\$rc
    exit \$rc
  "
echo "--- harness per-test result ---"
docker exec "$ctr" bash -lc "cd $mp/php-src && for t in $tests; do
  b=\"\${t%.phpt}\"
  if [ -f \"\$b.diff\" ]; then echo \"FAIL \$t\"; echo '=== diff ==='; cat \"\$b.diff\";
  elif [ -f \"\$b.skip\" ] || grep -q \"SKIP \$t\" /out/php-${_mode}-phpt.log; then echo \"SKIP \$t\";
  else echo \"PASS \$t\"; fi
done"
