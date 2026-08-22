#!/usr/bin/env bash
#
# court-runner.sh - ABI probe compilation and execution runner.
#
# Court Casefile: ABI-RUNNER-0001
# Description:   Compiles each C probe program with gcc and g++ (when
#                available), links against system libxml2 for oracle
#                mode or compiles against our headers for candidate
#                mode, runs each probe, and summarises pass/fail.
#
# Usage:
#   ./court-runner.sh                     # default: compile + run
#   ./court-runner.sh --oracle            # link system libxml2, run
#   ./court-runner.sh --candidate         # compile-only with our headers
#   ./court-runner.sh --oracle --candidate  # both modes
#
# Default (no flags):
#   Compile with our headers (no link), print struct sizes.
#
# Output:
#   Structured JSON-like per-probe results, final summary.

set -euo pipefail

# ------------------------------------------------------------------ #
#  Paths                                                             #
# ------------------------------------------------------------------ #
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
INCLUDE_DIR="${PROJECT_DIR}/include"

PROBES=(
    "ABI-STRUCT-NODE-0001-abicheck"
    "ABI-SYMBOL-0001-symbolcheck"
    "ABI-ENUM-0001-enumcheck"
)

CC_LIST=()
CC_COUNT=0

# ------------------------------------------------------------------ #
#  Flags                                                             #
# ------------------------------------------------------------------ #
MODE_ORACLE=false
MODE_CANDIDATE=false

for arg in "$@"; do
    case "$arg" in
        --oracle)    MODE_ORACLE=true    ;;
        --candidate) MODE_CANDIDATE=true ;;
        *)
            echo "Usage: $0 [--oracle] [--candidate]" >&2
            exit 1
            ;;
    esac
done

# Default: candidate-only
if ! $MODE_ORACLE && ! $MODE_CANDIDATE; then
    MODE_CANDIDATE=true
fi

# ------------------------------------------------------------------ #
#  Detect compilers                                                  #
# ------------------------------------------------------------------ #
detect_compilers() {
    if command -v gcc &>/dev/null; then
        CC_LIST+=("gcc")
        CC_COUNT=$((CC_COUNT + 1))
    fi
    if command -v g++ &>/dev/null; then
        CC_LIST+=("g++")
        CC_COUNT=$((CC_COUNT + 1))
    fi
    if [ "$CC_COUNT" -eq 0 ]; then
        echo "ERROR: no C compiler (gcc/g++) found" >&2
        exit 1
    fi
}

# ------------------------------------------------------------------ #
#  Return the appropriate -std flag for a compiler                   #
# ------------------------------------------------------------------ #
std_flag() {
    case "$1" in
        gcc) echo "-std=c11"  ;;
        g++) echo "-std=c++11" ;;
        *)   echo "-std=c11"   ;;
    esac
}

# ------------------------------------------------------------------ #
#  Build and run a probe in oracle mode                              #
# ------------------------------------------------------------------ #
run_oracle() {
    local probe="$1"
    local src="${SCRIPT_DIR}/${probe}.c"

    if [ ! -f "$src" ]; then
        echo "  { \"probe\": \"${probe}\", \"mode\": \"oracle\", \"status\": \"SKIP\", \"reason\": \"source not found\" },"
        return
    fi

    # Discover system libxml2 include path
    local sys_inc=""
    if command -v pkg-config &>/dev/null; then
        sys_inc=$(pkg-config --cflags libxml-2.0 2>/dev/null || true)
    fi
    if [ -z "$sys_inc" ] && [ -d "/usr/include/libxml2" ]; then
        sys_inc="-I/usr/include/libxml2"
    fi
    if [ -z "$sys_inc" ] && [ -d "/usr/local/include/libxml2" ]; then
        sys_inc="-I/usr/local/include/libxml2"
    fi

    for cc in "${CC_LIST[@]}"; do
        local std_opt
        std_opt="$(std_flag "$cc")"
        local bin_cc="/tmp/${probe}-oracle-${cc}"
        local errlog="/tmp/errlog-${probe}-${cc}-oracle.txt"
        local runlog="/tmp/runlog-${probe}-${cc}-oracle.txt"

        echo "  { \"probe\": \"${probe}\", \"mode\": \"oracle\", \"compiler\": \"${cc}\", \"build\": "
        # For enum check, use upstream expected values
        local extra_flags=""
        if [ "$probe" = "ABI-ENUM-0001-enumcheck" ]; then
            extra_flags="-DVERIFY_UPSTREAM"
        fi
        if "$cc" ${std_opt} -Wall -Wextra -Werror \
                -Wno-deprecated-declarations \
                ${sys_inc} \
                ${extra_flags} \
                -o "$bin_cc" "$src" \
                -lxml2 -lxslt 2>"$errlog"; then
            echo -n "\"ok\", \"run\": "
            if "$bin_cc" 2>"$runlog"; then
                echo ", \"result\": \"PASS\" },"
            else
                local rc=$?
                echo ", \"result\": \"FAIL (exit ${rc})\" },"
            fi
        else
            echo "\"FAIL\" },"
            cat "$errlog"
        fi
    done
}

# ------------------------------------------------------------------ #
#  Build a probe in candidate mode (compile-only, no link)            #
# ------------------------------------------------------------------ #
run_candidate() {
    local probe="$1"
    local src="${SCRIPT_DIR}/${probe}.c"

    if [ ! -f "$src" ]; then
        echo "  { \"probe\": \"${probe}\", \"mode\": \"candidate\", \"status\": \"SKIP\", \"reason\": \"source not found\" },"
        return
    fi

    for cc in "${CC_LIST[@]}"; do
        local std_opt
        std_opt="$(std_flag "$cc")"
        local obj="/tmp/${probe}-candidate-${cc}.o"
        local errlog="/tmp/errlog-${probe}-${cc}-candidate.txt"

        echo "  { \"probe\": \"${probe}\", \"mode\": \"candidate\", \"compiler\": \"${cc}\", \"build\": "
        if "$cc" ${std_opt} -Wall -Wextra -Werror \
                -I "${INCLUDE_DIR}" \
                -c "$src" \
                -o "$obj" \
                2>"$errlog"; then
            echo "\"ok\", \"result\": \"PASS\" },"
        else
            echo "\"FAIL\" },"
            cat "$errlog"
        fi
    done
}

# ------------------------------------------------------------------ #
#  main                                                              #
# ------------------------------------------------------------------ #
main() {
    detect_compilers

    echo "{"
    echo "  \"runner\": \"court-runner.sh\","
    echo "  \"probes\": ["

    local first=true
    for probe in "${PROBES[@]}"; do
        $first || echo ","
        first=false

        if $MODE_ORACLE && $MODE_CANDIDATE; then
            # Both modes: run oracle first, then candidate
            echo -n "  [ "
            run_oracle "$probe"
            echo -n "    "
            run_candidate "$probe"
            echo "  ]"
        elif $MODE_ORACLE; then
            run_oracle "$probe"
        elif $MODE_CANDIDATE; then
            run_candidate "$probe"
        fi
    done

    echo "  ],"

    # Summary
    local passed=0
    local failed=0
    local skipped=0

    if $MODE_CANDIDATE; then
        for probe in "${PROBES[@]}"; do
            for cc in "${CC_LIST[@]}"; do
                local o="/tmp/${probe}-candidate-${cc}.o"
                if [ -f "$o" ]; then
                    passed=$((passed + 1))
                else
                    failed=$((failed + 1))
                fi
            done
        done
    fi

    if $MODE_ORACLE; then
        for probe in "${PROBES[@]}"; do
            for cc in "${CC_LIST[@]}"; do
                local b="/tmp/${probe}-oracle-${cc}"
                if [ -x "$b" ]; then
                    if "$b" >/dev/null 2>&1; then
                        passed=$((passed + 1))
                    else
                        failed=$((failed + 1))
                    fi
                else
                    skipped=$((skipped + 1))
                fi
            done
        done
    fi

    echo "  \"summary\": {"
    echo "    \"passed\": ${passed},"
    echo "    \"failed\": ${failed},"
    echo "    \"skipped\": ${skipped}"
    echo "  }"
    echo "}"

    # Clean up temp files
    for probe in "${PROBES[@]}"; do
        rm -f "/tmp/${probe}"-*-*.o "/tmp/${probe}"-*-* 2>/dev/null || true
    done
    rm -f /tmp/errlog-*.txt /tmp/runlog-*.txt 2>/dev/null || true

    return "$failed"
}

main "$@"
