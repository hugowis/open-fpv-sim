#!/usr/bin/env bash
# Builds the pinned Betaflight SITL natively on Windows, in an MSYS2 "msys" shell (no WSL). Experimental, issue #67.
# Usage (MSYS2 shell, after `pacman -S base-devel git patch diffutils python gcc make`):
#   scripts/build-sitl-windows.sh      (env: BF_TAG, BF_SHA, BF_DIR, OFS_SITL_FLAGS, OFS_SITL_PATCH, OFS_SITL_WIN_PATCH)
# The result is <BF_DIR>/obj/main/betaflight_SITL.exe next to the Cygwin runtime DLLs, so it runs from any shell
# (point OFS_SITL_LAUNCH at it). See docs/research/sitl-native-windows.md.
# The Windows patch is made for the lockstep build (OFS_SITL_PATCH=none is refused), and BF_DIR must not contain spaces.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
BF_TAG="${BF_TAG:-2026.6.2}"
BF_SHA="${BF_SHA-e0b7bb01b17b21351057e9ead2d1ab39dd44fa16}"
BF_DIR="${BF_DIR:-$HOME/ofs/betaflight}"
FLAGS="${OFS_SITL_FLAGS:--DENABLE_GAZEBO_BRIDGE=0 -DENABLE_SIMULATOR_EXTERNAL_TIME=1}"
OFS_SITL_PATCH="${OFS_SITL_PATCH:-$REPO/third_party/betaflight/ofs-sitl.patch}"
OFS_SITL_WIN_PATCH="${OFS_SITL_WIN_PATCH:-$REPO/third_party/betaflight/ofs-sitl-windows.patch}"

if [ "$OFS_SITL_PATCH" = none ]; then
  echo "error: the Windows patch builds on the lockstep patch; OFS_SITL_PATCH=none is not supported here" >&2
  exit 1
fi
case "$BF_DIR" in
  *" "*) echo "error: BF_DIR ($BF_DIR) contains a space, which Betaflight's make cannot build in; set BF_DIR, e.g. BF_DIR=/c/ofs/betaflight" >&2; exit 1 ;;
esac

case "$(uname -s)" in
  MSYS_NT*|CYGWIN_NT*) ;;
  *) echo "error: run this from an MSYS2 (msys) shell on Windows; on Linux and WSL use scripts/build-sitl.sh" >&2; exit 1 ;;
esac

# Windows locks a running executable and its DLLs: the copies at the end would fail after the whole build.
if ps -W 2>/dev/null | grep -qi 'betaflight_SITL'; then
  echo "error: a betaflight_SITL.exe is running; stop it (or the simulator that started it) before building" >&2
  exit 1
fi
if [ ! -d "$BF_DIR/.git" ]; then
  git clone --branch "$BF_TAG" --depth 1 https://github.com/betaflight/betaflight "$BF_DIR"
fi
cd "$BF_DIR"
git reset -q --hard                # drop any previously applied patches before switching tags
git clean -q -fd src lib           # and the files the Windows patch adds
git fetch --depth 1 origin "refs/tags/$BF_TAG:refs/tags/$BF_TAG" 2>/dev/null || true
git checkout -q "$BF_TAG"
if [ -n "$BF_SHA" ] && [ "$(git rev-parse HEAD)" != "$BF_SHA" ]; then
  echo "error: $BF_TAG resolves to $(git rev-parse HEAD), expected pinned $BF_SHA" >&2
  exit 1
fi
git apply "$OFS_SITL_PATCH"
git apply "$OFS_SITL_WIN_PATCH"
git submodule update --init --recursive --depth 1
# Link-time optimisation is off: gcc 15's lto1 crashes on this link (internal compiler error in get_token).
# Betaflight's make does not track EXTRA_FLAGS or target.h edits reliably; build clean.
rm -rf obj/main/SITL
make TARGET=SITL OPTIMISATION_BASE="-ffast-math -fmerge-all-constants" EXTRA_FLAGS="$FLAGS" -j"$(nproc)"
# The Makefile names the executable *.elf; Windows wants .exe, and the Cygwin runtime DLL goes beside it.
cp obj/main/betaflight_SITL.elf obj/main/betaflight_SITL.exe
for dll in $(ldd obj/main/betaflight_SITL.exe | awk '$3 ~ /^\/usr\/bin\// { print $3 }'); do
  cp "$dll" obj/main/
done
echo "commit: $(git rev-parse HEAD)"
ls -l obj/main/betaflight_SITL.exe obj/main/*.dll
echo "In PowerShell: \$env:OFS_SITL_LAUNCH = '$(cygpath -w "$BF_DIR/obj/main/betaflight_SITL.exe")'"
