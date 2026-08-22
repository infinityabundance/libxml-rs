#!/bin/bash
# ── Oracle Sanity Test ───────────────────────────────────────────────────────
# Validates the Docker oracle container by running the built tools and checking
# that the installed libraries have the expected versions and SONAMEs.
#
# Usage:
#   ./oracle-test.sh [container-tag]
#
# The default container tag is libxml-rs/oracle:2.12.0.
#
# Produces a receipt file at:
#   courts/receipts/oracle-sanity-<tag>.txt
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
COURTS_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROJECT_DIR="$(cd "$COURTS_DIR/.." && pwd)"

CONTAINER_TAG="${1:-libxml-rs/oracle:2.12.0}"
RECEIPT_DIR="$COURTS_DIR/receipts"
RECEIPT_FILE="$RECEIPT_DIR/oracle-sanity-$(echo "$CONTAINER_TAG" | tr '/:' '--').txt"

# Sanitised tag for display
TAG_DISPLAY="$CONTAINER_TAG"

echo "================================================"
echo "  Oracle Sanity Test"
echo "  Container: ${TAG_DISPLAY}"
echo "================================================"
echo ""

# ── Check that Docker is available ──────────────────────────────────────────
if ! command -v docker &>/dev/null; then
    echo "ERROR: Docker is not available. This script requires Docker."
    echo "Install Docker and ensure the daemon is running."
    exit 1
fi

if ! docker info &>/dev/null; then
    echo "ERROR: Docker daemon is not running or not accessible."
    exit 1
fi

# ── Check that the image exists ─────────────────────────────────────────────
if ! docker image inspect "$CONTAINER_TAG" &>/dev/null; then
    echo "ERROR: Container image '${TAG_DISPLAY}' not found."
    echo "Build it first with:"
    echo "  docker build -t ${TAG_DISPLAY} \\"
    echo "    --build-arg LIBXML2_VERSION=2.12.0 \\"
    echo "    --build-arg LIBSXLT_VERSION=1.1.39 \\"
    echo "    -f docker/Dockerfile.oracle ."
    exit 1
fi

echo "Image found: ${TAG_DISPLAY}"
echo ""

# ── Helper: run command inside container ────────────────────────────────────
run() {
    docker run --rm "$CONTAINER_TAG" sh -c "$*"
}

# ── Gather version info ─────────────────────────────────────────────────────
echo "--- xmllint --version ---"
XMLLINT_VERSION=$(run xmllint --version 2>&1)
echo "$XMLLINT_VERSION"
echo ""

echo "--- xsltproc --version ---"
XSLTPROC_VERSION=$(run xsltproc --version 2>&1)
echo "$XSLTPROC_VERSION"
echo ""

echo "--- /oracle/VERSION ---"
ORACLE_VERSION=$(run cat /oracle/VERSION 2>&1)
echo "$ORACLE_VERSION"
echo ""

# ── Check SONAMEs ───────────────────────────────────────────────────────────
echo "--- Library SONAMEs ---"
LIBXML2_SONAME=$(run objdump -p /usr/local/lib/libxml2.so | grep SONAME)
echo "libxml2: $LIBXML2_SONAME"
LIBSXLT_SONAME=$(run objdump -p /usr/local/lib/libxslt.so | grep SONAME)
echo "libxslt: $LIBSXLT_SONAME"
LIBEXSLT_SONAME=$(run objdump -p /usr/local/lib/libexslt.so | grep SONAME)
echo "libexslt: $LIBEXSLT_SONAME"
echo ""

# ── Check library file listing ──────────────────────────────────────────────
echo "--- Library files ---"
LIB_FILES=$(run ls -la /usr/local/lib/libxml2* /usr/local/lib/libxslt* /usr/local/lib/libexslt* 2>&1)
echo "$LIB_FILES"
echo ""

# ── Run ldconfig / ldd verification ─────────────────────────────────────────
echo "--- Shared library linkage ---"
LDD_XML=$(run ldd /usr/local/lib/libxml2.so 2>&1 | head -20)
echo "libxml2.so linkage:"
echo "$LDD_XML"
echo ""
LDD_XSLT=$(run ldd /usr/local/lib/libxslt.so 2>&1 | head -20)
echo "libxslt.so linkage:"
echo "$LDD_XSLT"
echo ""

# ── Assert expected values ──────────────────────────────────────────────────
ERRORS=0

# Check xmllint version string contains expected libxml version
if echo "$XMLLINT_VERSION" | grep -q "libxml version 21200"; then
    echo "✓ xmllint reports libxml version 21200"
else
    echo "✗ xmllint version mismatch (expected 21200)"
    ERRORS=$((ERRORS + 1))
fi

# Check xsltproc version string contains expected libxslt version
if echo "$XSLTPROC_VERSION" | grep -q "libxslt 10139"; then
    echo "✓ xsltproc reports libxslt version 10139"
else
    echo "✗ xsltproc version mismatch (expected 10139)"
    ERRORS=$((ERRORS + 1))
fi

# Check SONAMEs
if echo "$LIBXML2_SONAME" | grep -q "libxml2.so.2"; then
    echo "✓ libxml2 SONAME is libxml2.so.2"
else
    echo "✗ libxml2 SONAME mismatch"
    ERRORS=$((ERRORS + 1))
fi

if echo "$LIBSXLT_SONAME" | grep -q "libxslt.so.1"; then
    echo "✓ libxslt SONAME is libxslt.so.1"
else
    echo "✗ libxslt SONAME mismatch"
    ERRORS=$((ERRORS + 1))
fi

if echo "$LIBEXSLT_SONAME" | grep -q "libexslt.so.0"; then
    echo "✓ libexslt SONAME is libexslt.so.0"
else
    echo "✗ libexslt SONAME mismatch"
    ERRORS=$((ERRORS + 1))
fi

echo ""

# ── Write receipt ───────────────────────────────────────────────────────────
mkdir -p "$RECEIPT_DIR"

cat > "$RECEIPT_FILE" <<RECEIPT_EOF
# Oracle Sanity Receipt
# Container: ${TAG_DISPLAY}
# Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
# Status: $([ "$ERRORS" -eq 0 ] && echo "PASS" || echo "FAIL (${ERRORS} errors)")
# ============================================================

## xmllint --version
${XMLLINT_VERSION}

## xsltproc --version
${XSLTPROC_VERSION}

## /oracle/VERSION
${ORACLE_VERSION}

## Library SONAMEs
libxml2: ${LIBXML2_SONAME}
libxslt: ${LIBSXLT_SONAME}
libexslt: ${LIBEXSLT_SONAME}

## Library files
${LIB_FILES}

## libxml2.so linkage
${LDD_XML}

## libxslt.so linkage
${LDD_XSLT}

## Assertions
$(echo "✓ xmllint reports libxml version 21200" && echo "$XMLLINT_VERSION" | grep -q "libxml version 21200" || echo "✗ FAIL")
$(echo "✓ xsltproc reports libxslt version 10139" && echo "$XSLTPROC_VERSION" | grep -q "libxslt 10139" || echo "✗ FAIL")
$(echo "✓ libxml2 SONAME is libxml2.so.2" && echo "$LIBXML2_SONAME" | grep -q "libxml2.so.2" || echo "✗ FAIL")
$(echo "✓ libxslt SONAME is libxslt.so.1" && echo "$LIBSXLT_SONAME" | grep -q "libxslt.so.1" || echo "✗ FAIL")
$(echo "✓ libexslt SONAME is libexslt.so.0" && echo "$LIBEXSLT_SONAME" | grep -q "libexslt.so.0" || echo "✗ FAIL")
RECEIPT_EOF

echo "Receipt written to: $RECEIPT_FILE"
echo ""

# ── Summary ─────────────────────────────────────────────────────────────────
if [ "$ERRORS" -eq 0 ]; then
    echo "================================================"
    echo "  ALL CHECKS PASSED"
    echo "================================================"
    exit 0
else
    echo "================================================"
    echo "  ${ERRORS} CHECK(S) FAILED"
    echo "================================================"
    exit 1
fi
