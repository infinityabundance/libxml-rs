#!/bin/bash
# ── Historical oracle builder ────────────────────────────────────────────
# Extracts a version tag from the archaeology git clone and builds it.
# Usage: build.sh <project: libxml2|libxslt> <tag> [libxml2-prefix]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/oracle/historical"
PROJ="$1"; TAG="$2"
LIBXML2_PREFIX="${3:-}"

# Resolve the git tag (era-dependent naming).
case "$PROJ" in
  libxml2) GIT="$ROOT/archaeology/libxml2-git";;
  libxslt) GIT="$ROOT/archaeology/libxslt-git";;
esac
# Normalize tag: try exact, then era spellings (v2.7.8, LIBXML2.6.32, LIBXML_1_8_17).
REV="$TAG"
if ! git -C "$GIT" rev-parse -q --verify "refs/tags/$REV" >/dev/null 2>&1; then
  REV="v$TAG"
fi
if ! git -C "$GIT" rev-parse -q --verify "refs/tags/$REV" >/dev/null 2>&1; then
  # 2.6.x-era spelling: version 2.6.32 -> tag LIBXML2.6.32 (major dropped)
  REV="LIBXML2.${TAG#2.}"
fi
if ! git -C "$GIT" rev-parse -q --verify "refs/tags/$REV" >/dev/null 2>&1; then
  # 0.x/1.x-era spelling: version 1.8.17 -> tag LIBXML_1_8_17
  REV="LIBXML_$(echo "$TAG" | tr '.' '_')"
fi
if ! git -C "$GIT" rev-parse -q --verify "refs/tags/$REV" >/dev/null 2>&1; then
  echo "ERROR: tag $TAG not found in $GIT (tried $REV)" >&2; exit 1
fi

SRC="$OUT/src/$PROJ-$TAG"
PREFIX="$OUT/prefix/$PROJ-$TAG"
mkdir -p "$SRC" "$PREFIX"
# Extract only tracked files (no submodules).
git -C "$GIT" archive "$REV" | tar -x -C "$SRC"

cd "$SRC"
# Git checkouts need autogen; release tarballs carry configure.
# Use the system autotools (the cargo bin shadows them with Rust shims).
export AUTORECONF=/usr/bin/autoreconf AUTOMAKE=/usr/bin/automake ACLOCAL=/usr/bin/aclocal
export AUTOHEADER=/usr/bin/autoheader AUTOCONF=/usr/bin/autoconf LIBTOOLIZE=/usr/bin/libtoolize
export PATH="/usr/bin:$PATH"
# Modernize era-incompatible autotools input (macros removed from modern
# autoconf/automake). Tracked as historical-build adaptations.
if [ -f configure.in ]; then
  sed -i 's/AM_CONFIG_HEADER/AC_CONFIG_HEADERS/' configure.in
  sed -i '/AM_C_PROTOTYPES/d; /AC_PROG_CC_STDC/d; /AM_EXEEXT/d' configure.in
fi
if [ ! -x ./configure ]; then
  if [ -x ./autogen.sh ]; then
    NOCONFIGURE=1 ./autogen.sh >/dev/null 2>&1 || /usr/bin/autoreconf -fi >/dev/null 2>&1
  else
    /usr/bin/autoreconf -fi >/dev/null 2>&1
  fi
fi

CFLAGS="-O2 -w" ./configure --prefix="$PREFIX" --disable-shared --enable-static \
  --without-python --without-http --without-ftp --without-icu --without-threads \
  ${LIBXML2_PREFIX:+--with-libxml-prefix=$LIBXML2_PREFIX} >/dev/null 2>&1
make -j"$(nproc)" >/dev/null 2>&1
make install >/dev/null 2>&1
echo "built $PROJ-$TAG -> $PREFIX"
