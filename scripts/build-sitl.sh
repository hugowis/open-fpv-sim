#!/usr/bin/env bash
# Builds the pinned Betaflight SITL used by Open FPV Sim.
# Usage: scripts/build-sitl.sh   (env: BF_TAG, BF_DIR, OFS_SITL_FLAGS, OFS_SITL_PATCH)
set -euo pipefail
BF_TAG="${BF_TAG:-2026.6.2}"
BF_DIR="${BF_DIR:-$HOME/ofs/betaflight}"
FLAGS="${OFS_SITL_FLAGS:--DENABLE_GAZEBO_BRIDGE=0}"

if [ ! -d "$BF_DIR/.git" ]; then
  git clone --branch "$BF_TAG" --depth 1 https://github.com/betaflight/betaflight "$BF_DIR"
fi
cd "$BF_DIR"
git fetch --depth 1 origin "refs/tags/$BF_TAG:refs/tags/$BF_TAG" 2>/dev/null || true
git checkout -q "$BF_TAG"
git checkout -q -- .            # drop any previously applied patch
if [ -n "${OFS_SITL_PATCH:-}" ]; then
  git apply "$OFS_SITL_PATCH"
fi
# The Makefile initialises submodules on demand; under -j that races on .git/config, so do it first.
rm -f .git/config.lock
git submodule update --init --recursive --depth 1
make TARGET=SITL EXTRA_FLAGS="$FLAGS" -j"$(nproc)"
echo "commit: $(git rev-parse HEAD)"
ls -l obj/main/betaflight_SITL.elf
