#!/bin/bash
# cand-phpt.sh — run ONE or a FEW phpt tests against the candidate on the
# phpC-volume container (phpbuild-c), printing per-test PASS/FAIL and the
# .diff bodies. Container must already exist (see cand-six-gate.sh).
#
# Usage: cand-phpt.sh <rel-phpt> [<rel-phpt> ...]
set -uo pipefail
REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
NAME="phpbuild-c"
CPUS="${CPUS:-14}"
tests="$*"
[ $# -ge 1 ] || { echo "usage: cand-phpt.sh <phpt...>"; exit 2; }

docker exec -e CPUS="$CPUS" "$NAME" bash -lc "
  set -uo pipefail
  source /court/consumers/lib.sh candidate
  source /court/consumers/php-court-spec.sh
  export MODE=candidate LOG=/out/cand-target.log CPUS=$CPUS
  cd /srcb
  /court/consumers/php-court-stage.sh candidate || exit \$?
"
echo "--- harness per-test result ---"
docker exec "$NAME" bash -lc "cd /srcb/php-src && for t in $tests; do
  b=\"\${t%.phpt}\"
  if [ -f \"\$b.diff\" ]; then echo \"FAIL \$t\"; echo '=== diff ==='; cat \"\$b.diff\";
  elif [ -f \"\$b.skip\" ] || grep -q \"SKIP \$t\" /out/cand-target.log; then echo \"SKIP \$t\";
  else echo \"PASS \$t\"; fi
done"
