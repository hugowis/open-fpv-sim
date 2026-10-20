#!/usr/bin/env bash
## Re-runs the ofs-fc slot unit tests in parallel to expose the macOS port-release flake.
## Usage: scripts/macos-slot-stress.sh [runs]   (default: 20)
set -euo pipefail
runs="${1:-20}"
i=1
while [ "$i" -le "$runs" ]; do
    echo "slot stress run $i/$runs"
    cargo test -p ofs-fc --lib sitl::slot::tests --locked -- --test-threads=8
    i=$((i + 1))
done
