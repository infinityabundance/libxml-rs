#!/bin/sh
# linker-wrapper.sh — libxml-rs custom linker (11.1-Z.1 three-DSO contract).
#
# Installed via .cargo/config.toml ([target.x86_64-unknown-linux-gnu] linker).
# Cargo invokes it for every link of the workspace. It passes the link
# straight through to `cc`, then — once the staticlib exists (the first
# bin/test/example link of a build; the lib target is always complete by
# then) — regenerates the libxslt/libexslt facade DSOs if they are stale.
#
# rustc's default linker is `cc`, so this adds no new toolchain requirement;
# cross builds and non-Linux targets are unaffected (the config key is
# target-specific and this script no-ops there).
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

out=
prev=
for a in "$@"; do
    if [ "$prev" = "-o" ]; then out="$a"; fi
    prev="$a"
done

cc "$@"
rc=$?
[ "$rc" -ne 0 ] && exit "$rc"

[ -n "$out" ] || exit 0
art="$(cd "$(dirname "$out")/.." 2>/dev/null && pwd)" || exit 0
if [ -f "$art/liblibxml_rs.a" ] && [ -f "$art/liblibxml_rs.so" ]; then
    "$SCRIPT_DIR/facade-gen.sh" "$art" >>"$art/.facade-gen.log" 2>&1
fi
exit 0
