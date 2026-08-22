#!/bin/bash
# ── Oracle Build Script ──────────────────────────────────────────────────
# Builds libxml2 + libxslt from extracted source tarballs.
# Called by Dockerfile.oracle during image build.
#
# Builds with a "distro-like" profile: maximum features, shared libraries,
# symbol versioning, and default toolchain. This matches the typical
# distribution build that downstream consumers expect.
#
# Usage: build.sh <libxml2-version> <libxslt-version>

set -exuo pipefail

LIBXML2_VERSION="${1:-2.15.3}"
LIBSXLT_VERSION="${2:-1.1.45}"
NPROC=$(nproc)

# ── Build libxml2 ────────────────────────────────────────────────────────
cd "/oracle/libxml2-${LIBXML2_VERSION}"

# Configure with maximal feature set matching a distro build.
# This enables: threads, HTTP, FTP, catalog, schemas, regexp, modules,
# XInclude, XPath, XPointer, C14N, HTML, reader, writer, DTD validation,
# RelaxNG, Schematron, ICU/iconv for encoding support.
./configure \
    --prefix=/usr/local \
    --enable-shared \
    --enable-static \
    --with-threads \
    --with-thread-alloc \
    --with-http \
    --with-ftp \
    --with-catalog \
    --with-schemas \
    --with-schematron \
    --with-regexps \
    --with-modules \
    --with-xinclude \
    --with-xpath \
    --with-xptr \
    --with-c14n \
    --with-html \
    --with-reader \
    --with-writer \
    --with-valid \
    --with-relaxng \
    --with-icu \
    --with-iconv \
    --with-zlib \
    --with-lzma \
    --with-python=no \
    --without-debug \
    --without-mem-debug \
    --without-run-debug \
    --with-output \
    --with-sax1 \
    --with-legacy \
    --with-schemas \
    --with-schematron \
    2>&1 | tee /oracle/libxml2-configure.log

make -j"${NPROC}" 2>&1 | tee /oracle/libxml2-build.log
make install 2>&1 | tee /oracle/libxml2-install.log

# Record build configuration for provenance
echo "=== libxml2 build config ===" > /oracle/libxml2-config.txt
cat config.h >> /oracle/libxml2-config.txt
echo "=== libxml2 config.status ===" >> /oracle/libxml2-config.txt
tail -100 config.status >> /oracle/libxml2-config.txt

# ── Build libxslt ────────────────────────────────────────────────────────
cd "/oracle/libxslt-${LIBSXLT_VERSION}"

# Configure against the just-built libxml2.
# Ensure pkg-config can find libxml2's .pc file
export PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:/usr/local/lib/x86_64-linux-gnu/pkgconfig
ldconfig 2>&1 || true
# Use --with-libxml-prefix so configure finds xml2-config in /usr/local/bin
# and uses it to set both CFLAGS and LIBS (including -lxml2).
# Do NOT use --with-libxml-include-prefix or --with-libxml-libs-prefix
# as those only set -I/-L without adding -lxml2 to the link line.
./configure \
    --prefix=/usr/local \
    --enable-shared \
    --enable-static \
    --with-libxml-prefix=/usr/local \
    --with-plugins \
    --without-debug \
    --without-mem-debug \
    --without-crypto \
    --without-python \
    2>&1 | tee /oracle/libxslt-configure.log

make -j"${NPROC}" 2>&1 | tee /oracle/libxslt-build.log
make install 2>&1 | tee /oracle/libxslt-install.log

# Record build configuration
echo "=== libxslt build config ===" > /oracle/libxslt-config.txt
cat config.h >> /oracle/libxslt-config.txt

# ── Post-build verification ──────────────────────────────────────────────
echo "=== Installed artifacts ==="
ls -la /usr/local/lib/libxml2* /usr/local/lib/libxslt* 2>&1 || true
ls -la /usr/local/bin/xmllint /usr/local/bin/xsltproc 2>&1 || true

echo "=== Library verification ==="
ldconfig 2>&1 || true
ldd /usr/local/lib/libxml2.so 2>&1 | head -20 || true
ldd /usr/local/lib/libxslt.so 2>&1 | head -20 || true

echo "=== Version check ==="
/usr/local/bin/xmllint --version 2>&1 || true
/usr/local/bin/xsltproc --version 2>&1 || true

echo "=== Oracle build complete ==="
