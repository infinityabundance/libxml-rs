#!/usr/bin/env bash
#
# court-runner.sh — 11.1-H header-compile court.
#
# Court family: HEADER-COMPILE
#
# The candidate headers are the drop-in C interface. This court verifies:
#
#   1. every public header compiles ALONE (it must be self-contained);
#   2. every public header compiles together with the full include set;
#   3. inclusion works in C89, C99, C11 and C17 modes under gcc and clang;
#   4. C++ inclusion works (upstream headers support C++ consumers);
#   5. every function DECLARED in the headers is actually EXPORTED by the
#      candidate DSO (no "present in the header but never compiled/exported"
#      API — the header is honest by construction);
#   6. warnings-as-errors for candidate builds, except deprecation warnings
#      that upstream headers themselves produce.
#
# Evidence: courts/receipts/phase-11/header-compile-<ts>-receipt.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
INCLUDE_DIR="${PROJECT_DIR}/include"
TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-11"
mkdir -p "$RECEIPT_DIR"

# The complete public include set (both projects)
PUBLIC_HEADERS=(
    # libxml2
    libxml/c14n.h libxml/catalog.h libxml/chvalid.h libxml/debugXML.h
    libxml/dict.h libxml/encoding.h libxml/entities.h libxml/globals.h
    libxml/hash.h libxml/HTMLparser.h libxml/HTMLtree.h libxml/list.h
    libxml/nanoftp.h libxml/nanohttp.h libxml/parser.h
    libxml/parserInternals.h libxml/pattern.h libxml/relaxng.h
    libxml/SAX.h libxml/SAX2.h libxml/schemasInternals.h
    libxml/schematron.h libxml/threads.h libxml/tree.h libxml/uri.h
    libxml/valid.h libxml/xinclude.h libxml/xlink.h libxml/xmlautomata.h
    libxml/xmlerror.h libxml/xmlexports.h libxml/xmlIO.h
    libxml/xmlmemory.h libxml/xmlmodule.h libxml/xmlreader.h
    libxml/xmlregexp.h libxml/xmlsave.h libxml/xmlschemas.h
    libxml/xmlschemastypes.h libxml/xmlstring.h libxml/xmlunicode.h
    libxml/xmlversion.h libxml/xmlwriter.h libxml/xpath.h
    libxml/xpathInternals.h libxml/xpointer.h
    # libxslt
    libxslt/attributes.h libxslt/documents.h libxslt/extensions.h
    libxslt/extra.h libxslt/functions.h libxslt/imports.h libxslt/keys.h
    libxslt/libxslt.h libxslt/namespaces.h libxslt/numbersInternals.h
    libxslt/pattern.h libxslt/preproc.h libxslt/security.h
    libxslt/templates.h libxslt/transform.h libxslt/trio.h
    libxslt/triodef.h libxslt/variables.h libxslt/xslt.h
    libxslt/xsltconfig.h libxslt/xsltInternals.h libxslt/xsltlocale.h
    libxslt/xsltexports.h libxslt/xsltutils.h
)

COMPILERS=(gcc clang)
STD_MODES_C=(-std=c89 -std=c99 -std=c11 -std=c17)
CXX_STD=-std=c++17

results=()
pass=0
fail=0
declare -A FAILED_MSGS

record() {
    # record <status> <label> [msg]
    results+=("{\"item\":\"$2\",\"status\":\"$1\"}")
    if [ "$1" = "PASS" ]; then pass=$((pass+1)); else fail=$((fail+1)); FAILED_MSGS["$2"]="${3:-}"; fi
}

sanitize() {
    # strip double quotes and control chars from a message for JSON embedding
    echo "$1" | tr -d '"\r' | tr '\n' ' '
}

# ------------------------------------------------------------------ #
# 1 + 2 + 3 + 4: compile each header alone and all together           #
# ------------------------------------------------------------------ #
for hdr in "${PUBLIC_HEADERS[@]}"; do
    src="/tmp/hdr-$(echo "$hdr" | tr '/' '_').c"
    printf '#include <%s>\nint main(void){return 0;}\n' "$hdr" > "$src"
    for cc in "${COMPILERS[@]}"; do
        if ! command -v "$cc" >/dev/null 2>&1; then continue; fi
        for std in "${STD_MODES_C[@]}"; do
            label="alone:$hdr:$cc:$std"
            if "$cc" $std -Wall -Wextra -Werror -Wno-deprecated-declarations \
                    -Wno-deprecated -I "$INCLUDE_DIR" -fsyntax-only "$src" \
                    >/tmp/hdr-err.txt 2>&1; then
                record PASS "$label"
            else
                record FAIL "$label" "$(sanitize "$(head -4 /tmp/hdr-err.txt)")"
            fi
        done
    done
done

# all-together compile
allsrc="/tmp/hdr-all.c"
{
    echo "#include <stddef.h>"
    for hdr in "${PUBLIC_HEADERS[@]}"; do echo "#include <$hdr>"; done
    echo "int main(void){return 0;}"
} > "$allsrc"
for cc in "${COMPILERS[@]}"; do
    if ! command -v "$cc" >/dev/null 2>&1; then continue; fi
    for std in "${STD_MODES_C[@]}"; do
        label="all-together:$cc:$std"
        if "$cc" $std -Wall -Wextra -Werror -Wno-deprecated-declarations \
                -Wno-deprecated -I "$INCLUDE_DIR" -fsyntax-only "$allsrc" \
                >/tmp/hdr-err.txt 2>&1; then
            record PASS "$label"
        else
            record FAIL "$label" "$(sanitize "$(head -4 /tmp/hdr-err.txt)")"
        fi
    done
    # C++ inclusion
    label="cxx:$cc:$CXX_STD"
    if "$cc" $CXX_STD -Wall -Wextra -Werror -Wno-deprecated-declarations \
            -Wno-deprecated -I "$INCLUDE_DIR" -fsyntax-only -x c++ "$allsrc" \
            >/tmp/hdr-err.txt 2>&1; then
        record PASS "$label"
    else
        record FAIL "$label" "$(sanitize "$(head -4 /tmp/hdr-err.txt)")"
    fi
done

# ------------------------------------------------------------------ #
# 5: declared-but-not-exported check against the candidate DSO        #
# ------------------------------------------------------------------ #
DSO="${PROJECT_DIR}/target/debug/liblibxml_rs.so"
dso_symbols=""
if [ -f "$DSO" ]; then
    dso_symbols="$(nm -D --defined-only "$DSO" 2>/dev/null | awk '{print $3}' | grep -E '^(xml|html|xslt|exslt)' | sort -u)"
fi
missing_syms=0
missing_list=""
for hdr in "${PUBLIC_HEADERS[@]}"; do
    [ -f "${INCLUDE_DIR}/${hdr}" ] || continue
    decls=$(grep -oE '^\s*(XMLPUBFUN|LIBXSLT_PUBLIC|XSLTPUBFUN)[^;]+;' "${INCLUDE_DIR}/${hdr}" \
            | grep -oE '\b(xml|html|xslt|exslt)[A-Za-z0-9_]+\s*\(' \
            | sed 's/[ (]//g' | sort -u)
    while read -r fn; do
        [ -z "$fn" ] && continue
        if ! echo "$dso_symbols" | grep -qx "$fn"; then
            missing_syms=$((missing_syms+1))
            missing_list="${missing_list}${hdr}:${fn} "
        fi
    done <<< "$decls"
done
if [ "$missing_syms" -eq 0 ]; then
    record PASS "declared-functions-exported"
else
    record FAIL "declared-functions-exported" "$missing_syms missing: $missing_list"
fi

# ------------------------------------------------------------------ #
# receipt                                                             #
# ------------------------------------------------------------------ #
RECEIPT="${RECEIPT_DIR}/header-compile-${TS}-receipt.json"
{
    echo "{"
    echo "  \"court\": \"HEADER-COMPILE\","
    echo "  \"phase\": \"11.1-H\","
    echo "  \"timestamp\": \"$TS\","
    echo "  \"toolchain\": {"
    first=1
    for cc in "${COMPILERS[@]}"; do
        if command -v "$cc" >/dev/null 2>&1; then
            if [ $first -eq 1 ]; then
                first=0
            else
                echo ","
            fi
            echo "    \"$cc\": \"$("$cc" --version | head -1)\""
        fi
    done
    echo ""
    echo "  },"
    echo "  \"headers_under_test\": ${#PUBLIC_HEADERS[@]},"
    echo "  \"summary\": { \"passed\": $pass, \"failed\": $fail },"
    echo "  \"results\": ["
    for i in "${!results[@]}"; do
        sep=","
        [ "$i" -eq $(( ${#results[@]} - 1 )) ] && sep=""
        echo "    ${results[$i]}$sep"
    done
    echo "  ],"
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
