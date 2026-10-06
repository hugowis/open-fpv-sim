#!/usr/bin/env bash
## Runs the Godot pilot client's headless tests: the unit suites and the open-loop end-to-end.
## Usage: scripts/run-godot-tests.sh [unit|e2e|all]   (default: all)
##
## Needs a Godot 4.7 binary: set GODOT_BIN, or the script downloads Godot 4.7.2 (the official release
## zip, SHA-512 verified) into build/godot-dl/ and uses that. The server and the extension must be
## built first (`cargo build -p ofs-sim -p ofs-godot`); the script builds them if the binary is missing.
##
## Godot's exit codes around broken test scripts (checked on 4.7.2): a script with a parse error exits
## 1, but a script that raises a runtime error never exits - the engine keeps running. Every Godot
## invocation here is therefore wrapped in `timeout -k`, and a kill fails the run.

set -euo pipefail
cd "$(dirname "$0")/.."

GODOT_VERSION="4.7.2"
GODOT_ASSET="Godot_v${GODOT_VERSION}-stable_linux.x86_64.zip"
GODOT_DL="${OFS_GODOT_DL:-build/godot-dl}"

suite="${1:-all}"

# The mode is selected here so an unknown one fails loudly before anything runs; it would
# otherwise run no suite at all and still print "Godot tests passed.".
case "$suite" in
    unit|e2e|all) ;;
    *) echo "usage: scripts/run-godot-tests.sh [unit|e2e|all]"; exit 1 ;;
esac

if [ -z "${GODOT_BIN:-}" ]; then
    mkdir -p "$GODOT_DL"
    GODOT_BIN="$GODOT_DL/Godot_v${GODOT_VERSION}-stable_linux.x86_64"
    if [ ! -x "$GODOT_BIN" ]; then
        echo "Downloading Godot ${GODOT_VERSION} into $GODOT_DL ..."
        curl -sSL --retry 3 -o "$GODOT_DL/$GODOT_ASSET" \
            "https://github.com/godotengine/godot/releases/download/${GODOT_VERSION}-stable/$GODOT_ASSET"
        curl -sSL --retry 3 -o "$GODOT_DL/SHA512-SUMS.txt" \
            "https://github.com/godotengine/godot/releases/download/${GODOT_VERSION}-stable/SHA512-SUMS.txt"
        expected=$(grep "$GODOT_ASSET" "$GODOT_DL/SHA512-SUMS.txt" | grep -oE "[0-9a-f]{128}" | head -1)
        if [ -z "$expected" ]; then
            echo "FAILED: the SHA-512 list has no entry for $GODOT_ASSET"
            exit 1
        fi
        echo "$expected  $GODOT_DL/$GODOT_ASSET" | sha512sum --check --status \
            || { echo "FAILED: the Godot download did not match SHA512-SUMS.txt"; exit 1; }
        unzip -q -o "$GODOT_DL/$GODOT_ASSET" -d "$GODOT_DL"
        chmod +x "$GODOT_BIN"
    fi
fi
echo "Godot: $GODOT_BIN"

# The server binary the client starts, and the extension library the project loads.
if [ ! -x target/debug/ofs-sim ] && [ ! -x target/debug/ofs-sim.exe ]; then
    cargo build -p ofs-sim -p ofs-godot --locked
fi

# `timeout -k` because a test script with a runtime error leaves the engine running forever.
run_godot() {
    local seconds="$1"; shift
    local rc=0
    timeout -k 5 "$seconds" "$GODOT_BIN" "$@" || rc=$?
    if [ "$rc" -ne 0 ]; then
        echo "FAILED: Godot exited $rc (parse errors exit 1; a runtime error hangs until the ${seconds}s timeout)"
        return "$rc"
    fi
}

# Two imports: on Windows a fresh first import (no .godot/) segfaults at process exit AFTER
# completing all import work (rc=139; any godot-rust class registration during the first editor
# scan, Godot 4.7.2; warm imports are unaffected). The first import's rc is therefore ignored -
# its work still populates .godot/ - and the second must exit 0, so a genuinely broken extension
# still fails the gate.
run_godot 120 --headless --path godot --import \
    || echo "NOTE: the first import's exit code is ignored (fresh-checkout segfault at exit on Windows; its work populated .godot/)"
run_godot 120 --headless --path godot --import

if [ "$suite" = unit ] || [ "$suite" = all ]; then
    run_godot 120 --headless --path godot -s res://tests/run_tests.gd
fi

if [ "$suite" = e2e ] || [ "$suite" = all ]; then
    run_godot 180 --headless --path godot -s res://tests/e2e_open_loop.gd
    if [ -n "${OFS_SITL_LAUNCH:-}" ]; then
        run_godot 300 --headless --path godot -s res://tests/e2e_betaflight.gd
    else
        echo "SKIP: OFS_SITL_LAUNCH is not set, so there is no Betaflight SITL to fly"
    fi
fi

echo "Godot tests passed."
