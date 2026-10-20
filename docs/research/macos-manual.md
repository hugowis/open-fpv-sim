# macOS on a physical machine: manual checklist

The CI survey (`docs/research/macos.md`) runs on GitHub's macOS runners. It cannot answer whether the game opens a
window, renders with Metal, or reads a gamepad on a real Mac. This checklist does. Record the machine and the results
in the table; leave "not run" rather than guessing.

| item | macOS | Godot | result | evidence |
|---|---|---|---|---|
| Gatekeeper opens the downloaded editor | | | not run | |
| The game opens a window | | | not run | |
| The renderer is Metal | | | not run | |
| The frame rate is playable | | | not run | |
| A gamepad is connected and read | | | not run | |
| The Python package installs | | | not run | |
| `ofs-sim` builds from source | | | not run | |

## 1. Download the editor

Use the same pinned zip as `scripts/run-godot-tests.sh` (the `Darwin` case): `Godot_v4.7.2-stable_macos.universal.zip`.
Download it and the release's `SHA512-SUMS.txt` with a browser (so Gatekeeper sees the quarantine flag), then check it:

```bash
shasum -a 512 -c SHA512-SUMS.txt   # the line for the macOS zip, from the release page
unzip Godot_v4.7.2-stable_macos.universal.zip
xattr -l Godot.app
spctl --assess --type execute -vv Godot.app
```

Record: the quarantine attribute, the `spctl` verdict, and whether opening `Godot.app` from Finder showed the
"unidentified developer" prompt. If it did, the expected workaround is Control-click -> Open; say so and whether it
worked.

## 2. Window and renderer

```bash
export GODOT_BIN="$PWD/Godot.app/Contents/MacOS/Godot"
cargo build -p ofs-sim -p ofs-godot --locked
export OFS_SIM_BIN="$PWD/target/debug/ofs-sim"
# Either fly open loop, or set OFS_SITL_LAUNCH to the patched SITL built in step 3:
"$GODOT_BIN" --verbose --path godot -- --open-loop
```

Record: a window appeared; the verbose log's rendering driver line (Metal); whether the HUD and FPV view animate; a
rough frame rate from the HUD or `--verbose`; whether the client started `ofs-sim` itself (the HUD shows a flying state
rather than an "unavailable" tip). A window that opens but shows a black viewport is a fail. If the open-loop flag is
not accepted by this client version, run without `-- --open-loop` after setting `OFS_SITL_LAUNCH` and record the
difference.

## 3. Gamepad

With the editor and the game closed:

```bash
"$GODOT_BIN" --path godot -s res://tests/manual_input.gd
```

Then start the game and open the controls screen. Record: the probe's `joypads:` line, the pad's name and info, the
axes/buttons it reports (move a stick and press a button between runs to see them change), and whether the sticks move
the bars. A pad that is listed but does not move the controls is a fail. Note the connection (USB/Bluetooth).

## 4. Python and `ofs-sim`

```bash
python3 -m pip install -e "python[dev]"
python3 -c "import ofs; print(ofs.__version__)"
cargo build -p ofs-sim --locked
```

Record the Python version, the install result, and the build result. On Intel, `rerun-sdk` has no x86_64 wheel; record
that the `[dev]` extra skips it rather than calling the install a failure.

## 5. Findings

Copy the filled table and the short notes into `docs/research/macos.md` under "A physical Mac". If an item fails, open
an issue with the exact command and output; do not change the checklist to make it pass.
