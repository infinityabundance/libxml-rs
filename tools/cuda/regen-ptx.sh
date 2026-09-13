#!/bin/sh
# regen-ptx.sh — rebuild the §16.9 device image from struct_scan.cu and record
# its provenance (compiler version + SHA-256). Requires nvcc (CUDA toolkit);
# ordinary libxml-rs builds never run this.
#
# Usage: sh tools/cuda/regen-ptx.sh [nvcc-path]
set -eu
NVCC="${1:-${NVCC:-nvcc}}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DIR="$ROOT/src/xml/parser/scan/cuda"
cd "$DIR"
command -v "$NVCC" >/dev/null 2>&1 || { echo "nvcc not found: $NVCC" >&2; exit 1; }
"$NVCC" -ptx -arch=compute_80 -O3 -o struct_scan.ptx struct_scan.cu
"$NVCC" --version | tail -n +4 > .nvcc-version.tmp
sha256sum struct_scan.ptx | awk '{print $1}' > struct_scan.ptx.sha256
echo "regenerated:"
ls -l struct_scan.ptx struct_scan.ptx.sha256
cat .nvcc-version.tmp
rm -f .nvcc-version.tmp
