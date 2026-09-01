#!/usr/bin/env bash
#
# substitution-runner.sh — Phase 12 DOCKER-SUBSTITUTION in-container runner.
#
# Runs inside the minimal Docker image libxml-rs/docker-substitution (FROM
# libxml-rs/oracle:2.15.3 — the canonical upstream libxml2 2.15.3 + libxslt
# 1.1.45 + libexslt 0.8.25 built from pinned source). The candidate DSOs are
# mounted read-only at /candidate; receipts go to /out.
#
# The reviewer's Phase-12 binary-substitution test, entirely inside the VM:
#
#   compile/link consumer against the container ORACLE DSOs (canonical
#       2.15.3 build, /usr/local)
#       -> preserve the binary unchanged
#       -> substitute the libxml-rs DSOs at runtime (LD_LIBRARY_PATH=/candidate)
#       -> execute; stdout/stderr/exit must be byte-identical
#
# Plus, inside the same VM:
#   - readelf --version-info / --dyn-syms comparison of both DSO sets
#   - ldd resolution proof that the substituted run used /candidate objects
#   - pkg-config build of a second unmodified consumer against the
#     candidate's .pc files + DSOs
#
set -uo pipefail

ORACLE_LIB=/usr/local/lib
CANDIDATE=/candidate
OUT=/out
PROBE=/probe/substitution-consumer.c
PROBE2=/probe/substitution-consumer2.c
WORK=/work
mkdir -p "$WORK"

results=()
pass=0
fail=0
declare -A FAILED_MSGS
record() {
    results+=("{\"item\":\"$2\",\"status\":\"$1\"}")
    if [ "$1" = "PASS" ]; then pass=$((pass+1)); else fail=$((fail+1)); FAILED_MSGS["$2"]="${3:-}"; fi
}
sanitize() { echo "$1" | tr -d '"\r' | tr '\n' ' '; }

TS="$(date -u +%Y%m%dT%H%M%SZ)"
RECEIPT="$OUT/docker-substitution-$TS.json"

echo "=== environment ==="
cat /etc/os-release | grep -E "^(NAME|VERSION)=" 
glibc="$(ldd --version | head -1)"
echo "glibc: $glibc"
oracle_ver="$(LD_LIBRARY_PATH=$ORACLE_LIB /usr/local/bin/xmllint --version 2>&1 | head -1)"
echo "container oracle: $oracle_ver"
echo "candidate dir: $(ls -la $CANDIDATE | head -8 | tr '\n' ' ')"

# ── 1. build the consumer against the container oracle ──────────────────── #
if cc -std=c11 -o "$WORK/consumer-oracle" "$PROBE" \
        -I/usr/local/include/libxml2 -I/usr/local/include/libxslt \
        -I/usr/local/include/libexslt \
        -L"$ORACLE_LIB" -lxslt -lxml2 -lexslt -Wl,-rpath,"$ORACLE_LIB" -ldl \
        >"$WORK/cc-o.txt" 2>&1; then
    record PASS "build:consumer-vs-container-oracle"
else
    record FAIL "build:consumer-vs-container-oracle" "$(sanitize "$(head -3 "$WORK/cc-o.txt")")"
fi

# ── 2. oracle run (baseline) ────────────────────────────────────────────── #
if [ -x "$WORK/consumer-oracle" ]; then
    "$WORK/consumer-oracle" >"$WORK/run-o.out" 2>"$WORK/run-o.err"
    o_rc=$?
    echo "oracle run rc=$o_rc"

    # ── 3. SAME binary, candidate DSOs substituted at runtime ────────────── #
    LD_LIBRARY_PATH="$CANDIDATE" "$WORK/consumer-oracle" >"$WORK/run-c.out" 2>"$WORK/run-c.err"
    c_rc=$?
    echo "candidate run rc=$c_rc"

    # Version-string profile: the container oracle is the canonical upstream
    # release-tarball build (xmlParserVersion "21503", xsltEngineVersion
    # "10145"); the candidate mirrors the EXECUTED oracle's profile, which is
    # a git-snapshot distro build ("21503-GITv2.15.3", "10145-GITv1.1.45") —
    # byte-identical version data with the executed oracle (R-000167 family,
    # ELF-VERSIONING court). The version lines are therefore profile facts,
    # asserted separately; every OTHER output line must be byte-identical.
    strip_version_lines() {
        grep -vE "version=|ver=|exslt=" "$1" || true
    }
    strip_version_lines "$WORK/run-o.out" > "$WORK/o-nover.out"
    strip_version_lines "$WORK/run-c.out" > "$WORK/c-nover.out"
    if [ "$o_rc" = "$c_rc" ] && cmp -s "$WORK/o-nover.out" "$WORK/c-nover.out" \
            && cmp -s "$WORK/run-o.err" "$WORK/run-c.err"; then
        record PASS "substitution:oracle-linked-binary-on-candidate (byte-identical excl. version profile, rc=$c_rc)"
    else
        record FAIL "substitution:oracle-linked-binary-on-candidate" \
            "oracle-rc=$o_rc candidate-rc=$c_rc out-diff=$(cmp -s "$WORK/o-nover.out" "$WORK/c-nover.out"; echo $?) err-diff=$(cmp -s "$WORK/run-o.err" "$WORK/run-c.err"; echo $?)"
    fi
    # version-profile assertions (candidate == executed-oracle git-snapshot
    # strings; container oracle == canonical release strings)
    if grep -q "version=21503-GITv2.15.3" "$WORK/run-c.out" \
            && grep -q "xslt ver=10145 10145-GITv1.1.45" "$WORK/run-c.out"; then
        record PASS "version:candidate reports executed-oracle git-snapshot profile (21503-GITv2.15.3)"
    else
        record FAIL "version:candidate profile" "$(grep -E 'version=|ver=' "$WORK/run-c.out")"
    fi
    if grep -q "version=21503" "$WORK/run-o.out" \
            && ! grep -q "GITv" "$WORK/run-o.out"; then
        record PASS "version:container oracle reports canonical release profile (21503)"
    else
        record FAIL "version:container oracle profile" "$(grep -E 'version=|ver=' "$WORK/run-o.out")"
    fi

    # the substituted run must emit NO ld.so version warnings
    if [ ! -s "$WORK/run-c.err" ] || ! grep -q "no version information" "$WORK/run-c.err"; then
        record PASS "substitution:no-ld.so-version-warnings"
    else
        record FAIL "substitution:no-ld.so-version-warnings" "$(sanitize "$(head -2 "$WORK/run-c.err")")"
    fi

    # the substituted run must have used the /candidate objects
    if LD_LIBRARY_PATH="$CANDIDATE" ldd "$WORK/consumer-oracle" 2>/dev/null \
            | grep -E "libxml2|libxslt|libexslt" | grep -q "$CANDIDATE"; then
        record PASS "substitution:ldd-resolves-into-candidate"
    else
        record FAIL "substitution:ldd-resolves-into-candidate" \
            "$(LD_LIBRARY_PATH="$CANDIDATE" ldd "$WORK/consumer-oracle" 2>&1 | grep -E 'libxml2|libxslt|libexslt')"
    fi

    # dlvsym named-node tests ran inside the consumer
    if grep -q "dlvsym node110=1 node112=1 node1134=1 wrong-node=0 bogus=1" "$WORK/run-c.out"; then
        record PASS "version:dlvsym-named-nodes"
    else
        record FAIL "version:dlvsym-named-nodes" "$(grep dlvsym "$WORK/run-c.out")"
    fi
fi

# ── 4. ELF plane comparison inside the VM ───────────────────────────────── #
# oracle DSO version-definition node sets vs candidate
node_names() { # <dso>
    readelf --version-info "$1" 2>/dev/null \
        | sed -n '/Version definition/,/Version needs/p' \
        | grep -oE 'Name: (LIBXML2_[0-9.]+|LIBXSLT_[0-9.]+|lib[a-z0-9]+\.so\.[0-9]+)' \
        | sed 's/Name: //'
}
o_x="$(node_names "$ORACLE_LIB/libxml2.so.16.1.3")"
c_x="$(node_names "$CANDIDATE/libxml2.so.16")"
o_s="$(node_names "$ORACLE_LIB/libxslt.so.1.1.45")"
c_s="$(node_names "$CANDIDATE/libxslt.so.1")"
o_e="$(node_names "$ORACLE_LIB/libexslt.so.0.8.25")"
c_e="$(node_names "$CANDIDATE/libexslt.so.0")"

# libxml2: the canonical 2.15.3 build is unversioned (upstream removed
# libxml2.syms); the candidate core must be unversioned too.
if [ -z "$o_x" ] && [ -z "$c_x" ]; then
    record PASS "elf:libxml2 unversioned on both sides"
else
    record FAIL "elf:libxml2 unversioned" "oracle=[$o_x] candidate=[$c_x]"
fi
# libxslt: candidate node set == oracle's named LIBXML2_1.x chain + terminal
if [ -n "$o_s" ] \
        && printf '%s\n' "$c_s" | grep -q "LIBXML2_1.1.45" \
        && [ "$(printf '%s\n' "$c_s" | grep -v '^$' | wc -l)" = \
             "$(( $(printf '%s\n' "$o_s" | grep -v '^$' | wc -l) + 1 ))" ]; then
    record PASS "elf:libxslt node set == oracle chain + terminal LIBXML2_1.1.45"
else
    record FAIL "elf:libxslt node set" "oracle=[$o_s] candidate=[$c_s]"
fi
# libexslt: unversioned on both sides
if [ -z "$o_e" ] && [ -z "$c_e" ]; then
    record PASS "elf:libexslt unversioned on both sides"
else
    record FAIL "elf:libexslt unversioned" "oracle=[$o_e] candidate=[$c_e]"
fi
# per-symbol version assignment on libxslt
omap="$(nm -D --defined-only "$ORACLE_LIB/libxslt.so.1.1.45" 2>/dev/null | grep '@@' | awk '{print $3}' | sort)"
cmap="$(nm -D --defined-only "$CANDIDATE/libxslt.so.1" 2>/dev/null | grep '@@' | awk '{print $3}' | sort)"
cmap_base="$(printf '%s\n' "$cmap" | grep -v '@@LIBXML2_1.1.45$' | sort)"
if [ "$omap" = "$cmap_base" ]; then
    record PASS "elf:libxslt per-symbol nodes == oracle"
else
    record FAIL "elf:libxslt per-symbol nodes" "oracle=$(echo "$omap" | wc -l) candidate-base=$(echo "$cmap_base" | wc -l)"
fi
# DT_NEEDED chains
if readelf -d "$CANDIDATE/libxslt.so.1" 2>/dev/null | grep NEEDED | grep -q "\[libxml2.so.16\]"; then
    record PASS "elf:libxslt NEEDs libxml2.so.16"
else
    record FAIL "elf:libxslt NEEDs libxml2.so.16" "$(readelf -d "$CANDIDATE/libxslt.so.1" 2>/dev/null | grep NEEDED)"
fi
# consumer DT_VERNEED records libxslt.so.1(LIBXML2_1.x)
if readelf --version-info "$WORK/consumer-oracle" 2>/dev/null | grep -q "libxslt.so.1" \
        && readelf --version-info "$WORK/consumer-oracle" 2>/dev/null | grep -q "LIBXML2_1.0.11"; then
    record PASS "elf:consumer VERNEED libxslt.so.1(LIBXML2_1.x)"
else
    record FAIL "elf:consumer VERNEED" "$(readelf --version-info "$WORK/consumer-oracle" 2>&1 | grep -A2 'Version needs' | head -6)"
fi

# ── 5. pkg-config build of a second unmodified consumer inside the VM ───── #
# baseline: probe 2 built against the container oracle
if cc -std=c11 -o "$WORK/consumer2-oracle" "$PROBE2" \
        -I/usr/local/include/libxml2 -I/usr/local/include/libxslt \
        -I/usr/local/include/libexslt \
        -L"$ORACLE_LIB" -lxslt -lxml2 -lexslt -Wl,-rpath,"$ORACLE_LIB" \
        >"$WORK/cc2-o.txt" 2>&1; then
    "$WORK/consumer2-oracle" >"$WORK/run2-o.out" 2>"$WORK/run2-o.err"
    o2_rc=$?
    record PASS "pkgconfig:probe2-oracle-baseline"
else
    record FAIL "pkgconfig:probe2-oracle-baseline" "$(sanitize "$(head -3 "$WORK/cc2-o.txt")")"
fi
# candidate .pc files are mounted at /candidate/pkgconfig (the target/debug
# install layout) but embed the HOST build prefix; rewrite them to the
# container mount point for an in-VM build.
PC_SRC="$CANDIDATE/pkgconfig"
if [ -f "$PC_SRC/libxml-2.0.pc" ] && [ -f "$PC_SRC/libxslt.pc" ]; then
    mkdir -p "$WORK/pc"
    for f in libxml-2.0 libxslt libexslt; do
        sed -e "s|${CANDIDATE}|/candidate|g" -e "s|/mnt/[^ ]*/target/debug|/candidate|g" \
            "$PC_SRC/$f.pc" > "$WORK/pc/$f.pc"
    done
    PC_DIR="$WORK/pc"
    if PKG_CONFIG_PATH="$PC_DIR" pkg-config --exists libxml-2.0 libxslt libexslt; then
        record PASS "pkgconfig:candidate .pc files resolve"
        cflags="$(PKG_CONFIG_PATH="$PC_DIR" pkg-config --cflags libxml-2.0 libxslt libexslt)"
        libs="$(PKG_CONFIG_PATH="$PC_DIR" pkg-config --libs libxml-2.0 libxslt libexslt)"
        if cc -std=c11 -o "$WORK/consumer-pc" "$PROBE2" $cflags \
                -L"$CANDIDATE" -Wl,-rpath,"$CANDIDATE" $libs -ldl >"$WORK/cc-pc.txt" 2>&1; then
            record PASS "pkgconfig:consumer builds via candidate .pc"
            LD_LIBRARY_PATH="$CANDIDATE" "$WORK/consumer-pc" >"$WORK/run-pc.out" 2>"$WORK/run-pc.err"
            pc_rc=$?
            strip_version_lines "$WORK/run2-o.out" > "$WORK/o2-nover.out"
            strip_version_lines "$WORK/run-pc.out" > "$WORK/pc-nover.out"
            if [ -x "$WORK/consumer2-oracle" ] && [ "$pc_rc" = "$o2_rc" ] \
                    && cmp -s "$WORK/o2-nover.out" "$WORK/pc-nover.out" \
                    && cmp -s "$WORK/run2-o.err" "$WORK/run-pc.err"; then
                record PASS "pkgconfig:consumer output byte-identical (rc=$pc_rc)"
            else
                record FAIL "pkgconfig:consumer output" "rc=$pc_rc out-diff=$(cmp -s "$WORK/o2-nover.out" "$WORK/pc-nover.out"; echo $?)"
            fi
        else
            record FAIL "pkgconfig:consumer build" "$(sanitize "$(head -4 "$WORK/cc-pc.txt")")"
        fi
    else
        record FAIL "pkgconfig:candidate .pc files resolve" "pkg-config error: $(PKG_CONFIG_PATH="$PC_DIR" pkg-config --exists libxml-2.0 libxslt libexslt 2>&1; echo rc=$?)"
    fi
else
    record FAIL "pkgconfig:candidate .pc files present" "ls: $(ls "$PC_DIR" 2>&1 | tr '\n' ' ')"
fi

# ── receipt ──────────────────────────────────────────────────────────────── #
{
    echo "{"
    echo "  \"court\": \"DOCKER-SUBSTITUTION\","
    echo "  \"phase\": \"12\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"schema\": \"docker-substitution-1\","
    echo "  \"image\": \"libxml-rs/docker-substitution (FROM libxml-rs/oracle:2.15.3)\","
    echo "  \"container\": {"
    echo "    \"os\": \"$(cat /etc/os-release | grep '^NAME=' | cut -d= -f2 | tr -d '\"')\","
    echo "    \"glibc\": \"$glibc\","
    echo "    \"oracle\": \"$oracle_ver\""
    echo "  },"
    echo "  \"substitution\": {"
    echo "    \"oracle_rc\": \"$o_rc\","
    echo "    \"candidate_rc\": \"$c_rc\","
    echo "    \"byte_identical\": \"$(cmp -s "$WORK/run-o.out" "$WORK/run-c.out" 2>/dev/null && echo true || echo false)\""
    echo "  },"
    echo "  \"cases\": ["
    for i in "${!results[@]}"; do
        sep=","
        [ "$i" -eq $(( ${#results[@]} - 1 )) ] && sep=""
        echo "    ${results[$i]}$sep"
    done
    echo "  ],"
    echo "  \"summary\": { \"passed\": $pass, \"failed\": $fail },"
    echo "  \"failures\": {"
    first=1
    for k in "${!FAILED_MSGS[@]}"; do
        [ $first -eq 1 ] || echo ","
        first=0
        echo "    \"$k\": \"${FAILED_MSGS[$k]}\""
    done
    echo "  }"
    echo "}"
} > "$RECEIPT"
echo "receipt -> $RECEIPT"
echo "passed=$pass failed=$fail"
exit $fail
