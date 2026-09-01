#!/usr/bin/env bash
#
# court-runner.sh — Phase 12 DOCKER-SUBSTITUTION court (host orchestrator).
#
# Court family: DOCKER-SUBSTITUTION
#
# The reviewer's Phase-12 binary-substitution test, run ENTIRELY inside a
# minimal Docker VM so it is reproducible and nothing is installed on the
# host:
#
#   compile/link consumer against the ORACLE DSOs (canonical 2.15.3 built
#       from pinned source inside the VM)
#       -> preserve binary unchanged
#       -> substitute libxml-rs DSOs at runtime
#       -> execute; byte-identical output required
#
# plus the ELF VERDEF/VERNEED plane, dlvsym named-node tests, ldd
# resolution proof, and a pkg-config build plane — all inside the VM.
#
# Evidence: courts/receipts/phase-12/docker-substitution-<ts>.json
#
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"
ARTIFACT="${PROJECT_DIR}/target/debug"
RECEIPT_DIR="${PROJECT_DIR}/courts/receipts/phase-12"
IMAGE="libxml-rs/docker-substitution:1"

mkdir -p "$RECEIPT_DIR"

if ! command -v docker >/dev/null 2>&1; then
    echo "FAIL: docker not available on the host"
    exit 1
fi

# ── 1. build the oracle image if absent (docker/Dockerfile.oracle) ──────── #
if ! docker image inspect libxml-rs/oracle:2.15.3 >/dev/null 2>&1; then
    echo "building libxml-rs/oracle:2.15.3 (canonical source build)..."
    docker build -t libxml-rs/oracle:2.15.3 \
        -f "${PROJECT_DIR}/docker/Dockerfile.oracle" "${PROJECT_DIR}" \
        || { echo "FAIL: oracle image build"; exit 1; }
fi

# ── 2. build the court image ────────────────────────────────────────────── #
echo "building ${IMAGE}..."
docker build -t "$IMAGE" -f "${SCRIPT_DIR}/Dockerfile" "$SCRIPT_DIR" \
    || { echo "FAIL: court image build"; exit 1; }

# ── 3. run the substitution court inside the VM ─────────────────────────── #
# mount the artifact dir read-only (candidate DSOs + pkgconfig) and a host
# receipt dir at /out
docker run --rm \
    -v "${ARTIFACT}":/candidate:ro \
    -v "${RECEIPT_DIR}":/out \
    "$IMAGE" 2>&1 | tail -40
rc=${PIPESTATUS[0]}

echo "=== docker-substitution exit=$rc ==="
exit $rc
