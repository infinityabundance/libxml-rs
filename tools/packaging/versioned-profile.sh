#!/bin/sh
# versioned-profile.sh — R-000179 versioned-distro profile builder.
#
# Links a SECOND artifact from the freshly built staticlib: libxml2.so.2
# (SONAME libxml2.so.2 — the distro SONAME) carrying the upstream LIBXML2_2.x
# named-version graph (tools/packaging/libxml2-versioned.syms, derived from
# the authoritative distro DSO's .gnu.version tables), so binaries built
# against a VERSIONED distro libxml2 bind without ld.so 'no version
# information available' warnings.
#
# The PRIMARY artifact (libxml2.so.16, unversioned) is untouched — the
# executed 2.15.3 oracle is unversioned and every executed court pins that.
# The versioned profile is an ADDITIONAL drop-in for the distro .2 contract.
#
# Output: <artifact-dir>/versioned/libxml2.so.2.13.9 (SONAME libxml2.so.2)
#         <artifact-dir>/versioned/libxml2.so.2  -> libxml2.so.2.13.9
#         <artifact-dir>/versioned/libxml2.so    -> libxml2.so.2
#
# usage: versioned-profile.sh <artifact-dir>
set -u
ART="$1"
LIB="$ART/lib"
ARCHIVE="$ART/liblibxml_rs.a"
OUT_DIR="$ART/versioned"
OUT="$OUT_DIR/libxml2.so.2.13.9"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SYMS="$SCRIPT_DIR/libxml2-versioned.syms"

[ -f "$ARCHIVE" ] || exit 0
[ -f "$SYMS" ] || exit 0
command -v cc >/dev/null 2>&1 || exit 0

mkdir -p "$OUT_DIR"

needs_gen() {
    [ -f "$1" ] || return 0
    [ "$1" -nt "$ARCHIVE" ] || return 0
    [ "$1" -nt "$SYMS" ] || return 0
    return 1
}

if needs_gen "$OUT"; then
    cc -shared -Wl,-soname,libxml2.so.2 -Wl,--version-script="$SYMS" \
        -o "$OUT.$$.tmp" \
        -Wl,--whole-archive "$ARCHIVE" -Wl,--no-whole-archive \
        -Wl,--gc-sections \
        -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc \
        && mv -f "$OUT.$$.tmp" "$OUT" \
        || rm -f "$OUT.$$.tmp"
fi
# refresh the symlink chain (leftover wrong targets are replaced)
ln -sf libxml2.so.2.13.9 "$OUT_DIR/libxml2.so.2"
ln -sf libxml2.so.2 "$OUT_DIR/libxml2.so"
exit 0
