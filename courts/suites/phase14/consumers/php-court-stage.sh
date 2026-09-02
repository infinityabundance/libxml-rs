#!/bin/bash
# php-court-stage.sh — in-container Phase-14.3 PHP build+test stage.
#
# Usage (inside the phase14 image): php-court-stage.sh <oracle|candidate>
#
#   stage 1  extract   pristine php tarball into $PWD/php-src (if missing)
#   stage 2  configure using php-court-spec.sh argv (only if not configured
#             OR the recorded build-contract input hash changed for candidate)
#   stage 3  make
#   stage 4  make test on the six XML extension suites, output to the caller's
#             chosen log, preserving the PHP harness artifact universe
#
# Env (from php-oom-court.sh / php-phpt-court.sh / direct use):
#   MODE        oracle|candidate (positional $1 if unset)
#   PHP_SRC     path to php-src work dir      (default $PWD/php-src)
#   LOG         absolute path for `make test` (default /out/php-<mode>-full.log)
#   CFG_LOG      absolute path for configure   (default /out/php-<mode>-configure.log)
#   MAKE_LOG     absolute path for make        (default /out/php-<mode>-make.log)
#   FORCE_CLEAN  wipe PHP_SRC first            (CLEAN SEAL; default unset => iteration)
#   FORCE_CONF   force reconfigure             (default unset)
set -uo pipefail

MODE="${1:-${MODE:?mode required}}"
# single source of truth: consumer identity + configure argv + extension set
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/php-court-spec.sh"
PHP_SRC="${PHP_SRC:-$PWD/php-src}"
CFG_LOG="${CFG_LOG:-/out/php-${MODE}-configure.log}"
MAKE_LOG="${MAKE_LOG:-/out/php-${MODE}-make.log}"
LOG="${LOG:-/out/php-${MODE}-full.log}"

# Build-contract input hash (candidate). Any change invalidates a previous
# configure/build of an ITERATION tree so stale object/config state can never
# leak into evidence. Computed over candidate headers + pkg-config + config
# scripts + DSO SONAME/export identity + libxml/libxslt/libexslt version macros
# + the canonical configure argv. Directory pruning = *.tmp / build byproducts.
candidate_contract_hash() {
  if ! command -v /candidate/lib >/dev/null 2>&1 && [ ! -d /candidate ]; then
    echo "contract-hash-unavailable"
    return
  fi
  # Deterministic: sort all files under the candidate include+pkgconfig+lib dirs
  # (excluding transient .tmp linker temporaries) + a digest of this spec's argv.
  local cfg
  cfg=$(printf '%s\n' "${PHP_CONFIGURE_ARGS[@]}")
  {
    find /candidate/include /candidate/lib/pkgconfig /candidate/lib \
         -type f \( -name '*.h' -o -name '*.pc' -o -name '*.sh' -o -name '*.la' \
            -o -name '*.so' -o -name '*.so.*' -o -name '*.a' \) \
         ! -name '*.tmp' 2>/dev/null | sort
    printf '%s\n' "$cfg"
  } | sha256sum | cut -d' ' -f1
}

# stage 1: presence/cleanup/extraction of the pristine tree.
if [ "${FORCE_CLEAN:-}" = "1" ]; then
  rm -rf "$PHP_SRC"
fi
if [ ! -f "$PHP_SRC/configure" ]; then
  mkdir -p "$PHP_SRC"
  tar xf "/src/$PHP_TARBALL" -C "$PHP_SRC" --strip-components=1
fi

cd "$PHP_SRC"

# stage 2: configure decision.
need_configure=0
if [ ! -f config.status ]; then
  need_configure=1
elif [ "$MODE" = "candidate" ] && [ "${FORCE_CONF:-}" != "1" ]; then
  prev="/out/.php-${MODE}-contract.sha"
  now="$(candidate_contract_hash)"
  if [ ! -f "$prev" ] || [ "$(cat "$prev" 2>/dev/null)" != "$now" ]; then
    echo "  build-contract changed ($(cat "$prev" 2>/dev/null) -> $now): forced reconfigure" >&2
    need_configure=1
    FORCE_CONF=1
    echo "$now" > "$prev"
  elif [ "${FORCE_CONF:-}" = "1" ]; then
    echo "$now" > "$prev"
  fi
fi

if [ "$need_configure" = "1" ] || [ "${FORCE_CONF:-}" = "1" ]; then
  # A contract change means object files compiled against the old headers are
  # stale even if config.status exists. Clean object/config state first when
  # reconfiguring an ITERATION tree.
  make distclean >/dev/null 2>&1 || true
  rm -f config.status config.cache
  ./configure "${PHP_CONFIGURE_ARGS[@]}" > "$CFG_LOG" 2>&1 || { echo "configure failed (rc=$?)"; exit 4; }
  if [ "$MODE" = "candidate" ]; then candidate_contract_hash > "/out/.php-${MODE}-contract.sha" 2>/dev/null || true; fi
fi

# stage 3: make (skip if freshly present for iteration).
if [ ! -x sapi/cli/php ] || [ "${need_configure}" = "1" ] || [ "${FORCE_CONF:-}" = "1" ]; then
  make -j"${CPUS:-$(nproc)}" > "$MAKE_LOG" 2>&1 || { echo "make failed (rc=$?)"; exit 5; }
fi

# stage 4: run the suite. Clear stale harness byproducts first so the artifact
# universe reflects THIS run (fail-closed: every *.diff must map to a failure).
rm -f /out/.php-${MODE}-fail-list.txt
TESTS_RUN="${PHP_TESTS_LIST:-$PHP_TESTS_SPEC}"
EXT_DIRS_RUN=("${PHP_EXT_DIRS[@]}")
if [ -n "${PHP_TESTS_LIST:-}" ]; then
  EXT_DIRS_RUN=()
  t="${TESTS_RUN%% *}"; EXT_DIRS_RUN+=("${t%%/*}")
fi
find "${EXT_DIRS_RUN[@]}" \( -name '*.diff' -o -name '*.out' -o -name '*.exp' \) -delete 2>/dev/null || true
make test TESTS="$TESTS_RUN" NO_INTERACTION=1 REPORT_EXIT_STATUS=1 > "$LOG" 2>&1
echo "rc=$(printf '%s' "$?")"
