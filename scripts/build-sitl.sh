#!/usr/bin/env bash
# Builds the pinned Betaflight SITL used by Open FPV Sim.
# Usage: scripts/build-sitl.sh   (env: BF_TAG, BF_DIR, OFS_SITL_FLAGS, OFS_SITL_PATCH)
# Default: the Open FPV Sim lockstep build (external time, legacy bridge); see docs/research/sitl-interface.md.
# OFS_SITL_PATCH=none builds stock Betaflight SITL.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
BF_TAG="${BF_TAG:-2026.6.2}"
BF_DIR="${BF_DIR:-$HOME/ofs/betaflight}"
FLAGS="${OFS_SITL_FLAGS:--DENABLE_GAZEBO_BRIDGE=0 -DENABLE_SIMULATOR_EXTERNAL_TIME=1}"
OFS_SITL_PATCH="${OFS_SITL_PATCH:-$REPO/third_party/betaflight/ofs-sitl.patch}"

if [ ! -d "$BF_DIR/.git" ]; then
  git clone --branch "$BF_TAG" --depth 1 https://github.com/betaflight/betaflight "$BF_DIR"
fi
cd "$BF_DIR"
git fetch --depth 1 origin "refs/tags/$BF_TAG:refs/tags/$BF_TAG" 2>/dev/null || true
git checkout -q "$BF_TAG"
git checkout -q -- .            # drop any previously applied patch
if [ "$OFS_SITL_PATCH" != "none" ]; then
  git apply "$OFS_SITL_PATCH"
fi
# The Makefile initialises submodules on demand; under -j that races on .git/config, so do it first.
rm -f .git/config.lock
git submodule update --init --recursive --depth 1
# Betaflight's make does not rebuild objects after target.h changes; patches change it, so build clean.
rm -rf obj/main/SITL
make TARGET=SITL EXTRA_FLAGS="$FLAGS" -j"$(nproc)"
echo "commit: $(git rev-parse HEAD)"
ls -l obj/main/betaflight_SITL.elf
