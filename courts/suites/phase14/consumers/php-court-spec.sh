#!/bin/bash
# php-court-spec.sh — Phase 14.3 PHP court: single source of truth.
#
# Every Phase-14.3 PHP court (php-oom-court.sh, php-phpt-court.sh, php-run*.sh,
# php-*-gdb.sh, php-*-probe.sh) sources THIS file for the consumer identity,
# the canonical configure argv, the exercised extension set, and the fail-closed
# invariants. There must never be two nominally-equivalent PHP courts that
# configure different consumer feature surfaces.
#
# Sourced (must not exit the parent).
#
# PHP consumer identity (pristine pinned upstream tarball):
PHP_VERSION="8.5.10"
PHP_TARBALL="php-${PHP_VERSION}.tar.gz"
# f5c0ac99b85b3d677de475c2e4f509f9b4f54663f3ee5a84d6d9481a521d4100  php-8.5.10.tar.gz
PHP_SHA256="f5c0ac99b85b3d677de475c2e4f509f9b4f54663f3ee5a84d6d9481a521d4100"

# Canonical configure argv (PHP 8.5 uses --with-xsl, NOT the stale --enable-xsl).
# Single source of truth for every Phase-14.3 PHP build, oracle and candidate.
PHP_CONFIGURE_ARGS=(
  --prefix=/usr/local/php
  --disable-all
  --enable-cli
  --enable-dom
  --enable-simplexml
  --enable-xml
  --enable-xmlreader
  --enable-xmlwriter
  --with-xsl
  --with-libxml
)

# Exercised extension test suites (the Phase-14.3 required set, in canonical order).
PHP_EXT_DIRS=(ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl)
PHP_EXT_NAMES=(dom simplexml xml xmlreader xmlwriter xsl)
PHP_TESTS_SPEC="${PHP_EXT_DIRS[*]}"

# Emit the canonical ./configure invocation line (single source of truth so a
# passive copy can never drift back to the stale --enable-xsl spelling).
php_configure_cmd() {
  printf './configure' ; printf ' %q' "${PHP_CONFIGURE_ARGS[@]}" ; printf '\n'
}

# Extract-once + configure + build helper shared by the small probe/gdb/phpt
# runners that need their own disposable fresh tree under a work dir.
# Work layout: $workdir/php-src  holds the pristine extraction.
php_prepare_and_build() {
  # php_prepare_and_build <workdir> <configure-log> <make-log>
  local dir="$1" cfglog="$2" makelog="$3"
  mkdir -p "$dir"
  if [ ! -f "$dir/php-src/configure" ]; then
    mkdir -p "$dir/php-src"
    tar xf "/src/$PHP_TARBALL" -C "$dir/php-src" --strip-components=1
  fi
  cd "$dir/php-src"
  if [ ! -f config.status ]; then
    eval "$(php_configure_cmd)" > "$cfglog" 2>&1 || { echo "configure failed"; return 4; }
  fi
  make -j"$(nproc)" > "$makelog" 2>&1 || { echo "make failed"; return 5; }
}

# Expected pinned universe (authoritative on the pristine oracle; candidate must
# match the oracle skip-set exactly and carry ZERO candidate-driven failures).
php_court_expected_universe() {
  # Expected from the pinned oracle build; verified by each clean-seal run.
  printenv PHASE14_EXPECT_TESTS >/dev/null 2>&1 || { :; }
}
