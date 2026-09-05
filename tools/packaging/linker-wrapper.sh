#!/bin/sh
# linker-wrapper.sh — libxml-rs custom linker (11.1-Z.1 three-DSO contract).
#
# Installed via .cargo/config.toml ([target.x86_64-unknown-linux-gnu] linker).
# Cargo invokes it for every link of the workspace. It passes the link
# straight through to `cc`, then — once the staticlib exists (the first
# bin/test/example link of a build; the lib target is always complete by
# then) — regenerates the libxslt/libexslt facade DSOs if they are stale.
#
# Phase 12 (EXPORT-SURFACE-DISPOSITION): rustc emits its own version script
# for cdylibs (a temp `deps/rustciXXXXXX/list` listing every no_mangle
# symbol as an unversioned global). With two --version-script args GNU ld
# merges them — the rustc list wins the symbol assignment, defeating the
# exact export map. This wrapper rewrites that rustc-generated script path
# to the committed tools/packaging/libxml2.syms, so ld receives exactly one
# version script: the upstream LIBXML2_2.x named-version graph + terminal
# node, with INTERNAL_LEAK symbols hidden. Bins/tests/examples (rlib links)
# are unaffected (no cdylib script).
#
# rustc's default linker is `cc`, so this adds no new toolchain requirement;
# cross builds and non-Linux targets are unaffected (the config key is
# target-specific and this script no-ops there).
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

out=
prev=
args=""
for a in "$@"; do
    if [ "$prev" = "-o" ]; then out="$a"; fi
    prev="$a"
    case "$a" in
        -Wl,--version-script=*)
            vs="${a#-Wl,--version-script=}"
            # rustc's generated cdylib export list lives under deps/rustci*
            if printf '%s' "$vs" | grep -qi '/deps/rustc.*/list$'; then
                if [ -f "$SCRIPT_DIR/libxml2.syms" ]; then
                    a="-Wl,--version-script=$SCRIPT_DIR/libxml2.syms"
                fi
            fi
            ;;
    esac
    args="$args $a"
done

# shellcheck disable=SC2086
cc $args
rc=$?
[ "$rc" -ne 0 ] && exit "$rc"

[ -n "$out" ] || exit 0
art="$(cd "$(dirname "$out")/.." 2>/dev/null && pwd)" || exit 0
if [ -f "$art/liblibxml_rs.a" ] && [ -f "$art/liblibxml_rs.so" ]; then
    "$SCRIPT_DIR/facade-gen.sh" "$art" >>"$art/.facade-gen.log" 2>&1
    "$SCRIPT_DIR/versioned-profile.sh" "$art" >>"$art/.versioned-profile.log" 2>&1
fi
exit 0
