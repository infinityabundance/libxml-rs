#!/bin/sh
# regression.sh — fail-closed regression gate for the libxml-rs drop-in.
#
# The single source of truth for "did a change regress the library" is:
#   1. cargo test --lib  (unit + integration + proptest fuzz smoke)
#   2. cargo build --release --lib  (the three-DSO drop-in actually links)
#
# Any non-zero exit fails the gate. Run from the repo root.

set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

echo "==> cargo test --lib"
cargo test --lib

echo "==> cargo build --release --lib (drop-in DSO link)"
cargo build --release --lib

echo "==> regression gate PASSED"
