#!/bin/bash
# lib.sh — Phase 14 shared in-container environment setup.
#
# Usage: source lib.sh <mode>   where mode is `oracle` or `candidate`.
#
# oracle:     pkg-config/ld/CLI all resolve the canonical source-built
#             libxml2 2.15.3 + libxslt 1.1.45 in /usr/local.
# candidate:  the host `target/debug` is mounted read-only at /candidate;
#             container-local .pc files (prefix=/candidate) take precedence
#             so pkg-config resolves libxml-2.0/libxslt/libexslt to the
#             libxml-rs DSOs; LD_LIBRARY_PATH and PATH point at /candidate.
set -uo pipefail

MODE="${1:?usage: lib.sh <oracle|candidate>}"

case "$MODE" in
  oracle)
    export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig
    export LD_LIBRARY_PATH=/usr/local/lib
    export PATH=/usr/local/bin:/usr/bin:/bin
    ;;
  candidate)
    mkdir -p /cand-pc
    for f in /candidate/lib/pkgconfig/*.pc; do
      b="$(basename "$f")"
      sed 's|^prefix=.*|prefix=/candidate|' "$f" > "/cand-pc/$b"
    done
    export PKG_CONFIG_PATH=/cand-pc
    export LD_LIBRARY_PATH=/candidate/lib
    export PATH=/candidate/bin:/usr/bin:/bin
    ;;
  *)
    echo "lib.sh: unknown mode '$MODE'" >&2
    exit 2
    ;;
esac

# Deterministic build flags (identical on both sides).
export CFLAGS="-O1 -g0"
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export PYTHONIOENCODING=utf-8
export PYTHONUTF8=1
