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
    # build.rs bakes the HOST artifact path into each libtool `.la` libdir
    # (the candidate is built on the host at $REPO/target/debug but consumed
    # in-container at /candidate). libtool resolves `-lxml2`/`-lxslt`/`-lexslt`
    # through those `.la` files and, when the recorded libdir does not exist,
    # emits the host absolute path to the linker — which is absent in the
    # container and fails any relink. Make each recorded libdir true by
    # bridging it to the mounted /candidate/lib. Without this the PHP gate only
    # passes while `make` is a no-op; any candidate change forces a relink and
    # the link dies with "cannot find <host>/lib/libexslt.so".
    for f in /candidate/lib/*.la; do
      [ -f "$f" ] || continue
      d="$(sed -n "s/^libdir='\\(.*\\)'/\\1/p" "$f")"
      [ -n "$d" ] || continue
      [ -e "$d" ] && continue
      mkdir -p "$(dirname "$d")" 2>/dev/null || continue
      ln -sfn /candidate/lib "$d" 2>/dev/null || true
    done
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
