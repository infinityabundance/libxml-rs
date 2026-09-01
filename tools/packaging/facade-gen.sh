#!/bin/sh
# facade-gen.sh — libxml-rs three-DSO facade generator (11.1-Z.1).
#
# Cargo can only emit one cdylib per link, so the two facade DSOs are
# generated POST-LINK from the freshly linked staticlib:
#
#   lib/libxslt.so.1.1.45   SONAME libxslt.so.1 — exports the xslt* surface
#                           (version-script filtered), NEEDs libxml2.so.16.
#   lib/libexslt.so.0.8.25  SONAME libexslt.so.0 — exports the exslt* surface,
#                           NEEDs libxslt.so.1 and libxml2.so.16.
#
# The facades are whole-core re-links (--whole-archive of the staticlib), so
# every xslt/exslt export is a real definition — consumers link with plain
# `-lxslt`/`-lexslt` exactly as with upstream. The version scripts give each
# facade the oracle's per-DSO export surface (nm -D of /usr/lib/libxslt.so.1:
# xslt* + xslDebugStatus; /usr/lib/libexslt.so.0: exslt*) instead of leaking
# the whole combined core. `--no-as-needed -l:` records the upstream-faithful
# NEEDED chain. The staticlib members are native ELF objects in debug and
# release alike (release uses internal LTO with codegen-units=1).
#
# Idempotent (regenerates only when a facade is missing, is a leftover
# symlink, or is older than the core/archive) and flock-serialized for
# parallel bin links. On failure the existing facades (or the symlink
# fallback) remain — the build never hard-fails from this step.
#
# usage: facade-gen.sh <artifact-dir>
set -u
ART="$1"
LIB="$ART/lib"
CORE="$ART/liblibxml_rs.so"
ARCHIVE="$ART/liblibxml_rs.a"
XSLT_OUT="$LIB/libxslt.so.1.1.45"
EXSLT_OUT="$LIB/libexslt.so.0.8.25"

[ -f "$CORE" ] || exit 0
[ -f "$ARCHIVE" ] || exit 0
command -v cc >/dev/null 2>&1 || exit 0

# serialize concurrent wrapper invocations (parallel bin links)
if command -v flock >/dev/null 2>&1; then
    exec 9>"$ART/.facade-gen.lock"
    flock 9 2>/dev/null || exit 0
fi

needs_gen() {
    # a symlink at the facade path is never a real facade (leftover from
    # earlier build-script versions / inactive-machinery runs)
    [ -L "$1" ] && return 0
    [ -f "$1" ] || return 0
    [ "$1" -nt "$CORE" ] || return 0
    [ "$1" -nt "$ARCHIVE" ] || return 0
    # Phase 12: regenerate when the generated export maps / generator change
    for vs in "$SCRIPT_DIR/libxslt.syms" "$SCRIPT_DIR/libexslt.syms" \
              "$SCRIPT_DIR/../phase12/export_surface.py"; do
        [ -f "$vs" ] || continue
        [ "$1" -nt "$vs" ] || return 0
    done
    return 1
}

XSLT_VS="$(mktemp "$ART/.libxslt.vs.XXXXXX")"
EXSLT_VS="$(mktemp "$ART/.libexslt.vs.XXXXXX")"
TMP_VS=1
# Phase 12: the facades use the generated EXACT export maps with the
# oracle's named-version graph (libxslt.syms: the 27-node LIBXML2_1.x chain
# with the per-symbol node assignment of the executed oracle) instead of the
# pre-Phase-12 anonymous `xslt*` filter. libexslt.syms is the exact oracle
# surface (unversioned, matching the executed 0.8.25). Both hide every
# unlisted symbol (local: *), so xslt/exslt implementation internals that
# the oracle does not export no longer escape into the dynamic surface
# (EXPORT-SURFACE-DISPOSITION, Phase 12).
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$SCRIPT_DIR/libxslt.syms" ]; then
    rm -f "$XSLT_VS"
    XSLT_VS="$SCRIPT_DIR/libxslt.syms"
    TMP_VS=0
else
    cat > "$XSLT_VS" <<'VSEOF'
{
  global:
    xslt*;
    xslDebugStatus;
  local: *;
};
VSEOF
fi
if [ -f "$SCRIPT_DIR/libexslt.syms" ]; then
    rm -f "$EXSLT_VS"
    EXSLT_VS="$SCRIPT_DIR/libexslt.syms"
    TMP_VS=0
else
    cat > "$EXSLT_VS" <<'VSEOF'
{
  global:
    exslt*;
  local: *;
};
VSEOF
fi
if [ "$TMP_VS" = 1 ]; then
    trap 'rm -f "$XSLT_VS" "$EXSLT_VS"' EXIT
fi

GEN_OK=1
if needs_gen "$XSLT_OUT"; then
    cc -shared -Wl,-soname,libxslt.so.1 -Wl,--version-script="$XSLT_VS" \
        -o "$XSLT_OUT.$$.tmp" \
        -Wl,--whole-archive "$ARCHIVE" -Wl,--no-whole-archive \
        -Wl,--gc-sections \
        -Wl,--no-as-needed -L"$LIB" -l:libxml2.so.16.1.3 -Wl,--as-needed \
        -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc \
        && mv -f "$XSLT_OUT.$$.tmp" "$XSLT_OUT" \
        || GEN_OK=0
fi
if [ "$GEN_OK" = 1 ] && needs_gen "$EXSLT_OUT"; then
    cc -shared -Wl,-soname,libexslt.so.0 -Wl,--version-script="$EXSLT_VS" \
        -o "$EXSLT_OUT.$$.tmp" \
        -Wl,--whole-archive "$ARCHIVE" -Wl,--no-whole-archive \
        -Wl,--gc-sections \
        -Wl,--no-as-needed -L"$LIB" -l:libxslt.so.1.1.45 -l:libxml2.so.16.1.3 -Wl,--as-needed \
        -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc \
        && mv -f "$EXSLT_OUT.$$.tmp" "$EXSLT_OUT" \
        || GEN_OK=0
fi
exit 0
