#!/bin/bash
# consumers-setup.sh — §16.12 build/stage the five consumer runtimes inside the
# perf container, for both providers (oracle at /usr/local, candidate at /candidate).
#
# Idempotent: existing staged artifacts are reused. Run from the host:
#   sh courts/suites/phase16/consumers-setup.sh
set -uo pipefail
REPO="${REPO:-/mnt/1tb_kingston/libxml-rs}"
NAME="${PERF_CONTAINER:-perf-c}"
IMG="${IMG:-libxml-rs/phase14-debian:1}"

if ! docker ps --filter "name=^/${NAME}$" --format '{{.Names}}' | grep -qx "$NAME"; then
  docker rm -f "$NAME" >/dev/null 2>&1 || true
  mkdir -p "$REPO/target/perf-out"
  docker run -d --name "$NAME" --memory=100g --memory-swap=100g --cpus=16 \
    -v "$REPO/courts/suites/phase14/consumers:/court/consumers:ro" \
    -v "$REPO/tools/bench/consumers:/bench:ro" \
    -v "$REPO/target/release:/candidate:ro" \
    -v /tmp/lxmlrs-corpus/files:/corpus:ro \
    -v "$REPO/target/perf-out:/out" \
    "$IMG" sleep infinity >/dev/null
  echo "container $NAME (re)created"
fi

# 1. xsltperf (libxslt compile / precompiled-apply microdriver), per provider.
for MODE in oracle candidate; do
  docker exec "$NAME" bash -lc "
    set -e
    source /court/consumers/lib.sh $MODE
    cc -O2 /bench/xsltperf.c \$(pkg-config --cflags --libs libxslt libxml-2.0) \
       -o /out/xsltperf-$MODE
    echo 'built /out/xsltperf-$MODE'
  " || echo "xsltperf-$MODE build FAILED"
done

# 2. lxml (built from /src/lxml against each provider), per provider.
for MODE in oracle candidate; do
  if [ -e "$REPO/target/perf-out/lxml-$MODE/src/lxml/etree.so" ]; then
    echo "lxml-$MODE already built"
    continue
  fi
  docker exec "$NAME" bash -lc "
    set -e
    source /court/consumers/lib.sh $MODE
    rm -rf /out/lxml-$MODE
    cp -r /src/lxml /out/lxml-$MODE
    cd /out/lxml-$MODE
    python3 setup.py build_ext --inplace --force > /out/lxml-$MODE.build.log 2>&1
    echo 'built lxml-$MODE'
  " || echo "lxml-$MODE build FAILED (see /out/lxml-$MODE.build.log)"
done

# 3. nokogiri (Ruby): one build against the oracle headers; the provider is
#    selected at runtime by LD_LIBRARY_PATH (identical ABI sonames).
if [ ! -e "$REPO/target/perf-out/nokogiri/lib/nokogiri/nokogiri.so" ] \
   && ! docker exec "$NAME" test -e /src/nokogiri/lib/nokogiri/nokogiri.so; then
  docker exec "$NAME" bash -lc "
    set -e
    source /court/consumers/lib.sh oracle
    cd /src/nokogiri/ext/nokogiri
    NOKOGIRI_USE_SYSTEM_LIBRARIES=yes ruby extconf.rb --gumbo-dev > /out/nokogiri.extconf.log 2>&1
    make -j\"\$(nproc)\" > /out/nokogiri.make.log 2>&1
    cp nokogiri.so /src/nokogiri/lib/nokogiri/nokogiri.so
    echo 'built nokogiri'
  " || echo "nokogiri build FAILED (see /out/nokogiri.*.log)"
fi

# 4. PHP: staged by the Phase-14 harness. Verify presence; rebuild instructions
#    are in courts/suites/phase14/consumers/{php-court-stage.sh,cand-six-gate.sh}.
for p in php-oracle php; do
  if [ -x "$REPO/target/perf-out/$p" ]; then
    echo "php present: /out/$p"
  else
    echo "php MISSING: /out/$p — build via the Phase-14 harness (php-court-stage.sh)"
  fi
done

echo "consumers-setup done"
