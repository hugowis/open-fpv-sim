# Developer setup

## Requirements
- Rust stable ≥ 1.85 (`rustup`), Python ≥ 3.10.
- Betaflight SITL: Linux, or WSL2 (Ubuntu) on Windows. Build with `bash scripts/build-sitl.sh` (see the script for env options). It applies `third_party/betaflight/ofs-sitl.patch`: lockstep time, UART bytes in the state datagram, deterministic boot.

## Everyday commands
| What | Command |
|---|---|
| All Rust tests | `cargo test --workspace` |
| Live SITL tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` |
| Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` (Ctrl-C stops it and its SITL) |
| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` |
| Godot client tests | `bash scripts/run-godot-tests.sh` (see "Godot pilot client" below) |
| Real-time session (for Configurator) | `OFS_SITL_LAUNCH=<cmd> python python/examples/serve_realtime.py` |
| Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |
| Regenerate the SITL patch | start from the M1 patch (`third_party/betaflight/ofs-sitl.patch` at commit c1514e2) applied to the pinned Betaflight checkout, run `add_serial_in_datagram.py` then `deterministic_boot.py` (both in `third_party/betaflight/tools/`; see their docstrings). The generators are not idempotent: running them on a tree that already has the current patch inserts duplicates |

## Environment variables
- `OFS_SIM_BIN` — path to `ofs-sim` used by `ofs.launch()`.
- `OFS_SITL_LAUNCH` — SITL argv (space-separated, so no spaces inside paths), overrides `fc.launch` in quad files.
  Windows: `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`. Works with WSL's default NAT networking: the bridge discovers the WSL VM and host IPs and passes `--ip` (see `docs/research/sitl-interface.md` §6).
- `OFS_SITL_CLEANUP` — argv run before launch and after stop to kill stray SITL processes (also when a native SITL's UDP 9003 is still held). It defaults to `pkill -x betaflight_SITL` on Linux and to `<wsl prefix> pkill -x betaflight_SITL` under WSL (required there: a stale SITL would otherwise answer instead of the new one). It matches the process name: never use `pkill -f`, which also matches the shell running it.
- `OFS_SITL_HOST`, `OFS_SITL_REPLY_IP` — override the address state datagrams go to, and the address SITL replies to (`--ip`, motor socket bind).

## Godot pilot client
The game client lives in `godot/`; README.md describes what it is, its keys and its settings. It needs Godot **4.7.2** and the extension plus server built:

    cargo build -p ofs-sim -p ofs-godot

- **Install Godot:** download the official release zip (`Godot_v4.7.2-stable_win64.exe.zip`; Linux: `Godot_v4.7.2-stable_linux.x86_64.zip`) from the 4.7.2-stable release page and check it against that release's `SHA512-SUMS.txt`. For headless runs on Windows use the console binary from the same zip (`Godot_v4.7.2-stable_win64_console.exe`): the standard exe is a GUI program that detaches from the terminal, so its output is lost.
- **Tests:** `bash scripts/run-godot-tests.sh [unit|e2e|all]` runs the unit suites and the end-to-end tests. It downloads Godot 4.7.2 itself (SHA-512 checked, into `build/godot-dl/`, override with `OFS_GODOT_DL`) unless `GODOT_BIN` points at any 4.7 binary, and it always builds the server and extension first (a no-op when up to date). The Betaflight e2e runs only when `OFS_SITL_LAUNCH` is set.
- **Direct commands** (what the script wraps; both e2e scripts fly a real ofs-sim):
  - `godot --headless --path godot -s res://tests/run_tests.gd` — the unit suites;
  - `godot --headless --path godot -s res://tests/e2e_open_loop.gd` — open-loop e2e (no firmware);
  - `OFS_SITL_LAUNCH=<cmd> godot --headless --path godot -s res://tests/e2e_betaflight.gd` — e2e with real Betaflight (armed through CRSF, climbs, reload re-arms).
- **`timeout -k`:** a test script with a parse error exits 1, but a script that raises a runtime error never exits — the engine just keeps running. The runner therefore wraps every Godot call in `timeout -k` and treats a kill (rc 124) as a failure.
- **`CARGO_TARGET_DIR`:** the Godot editor and `godot --path godot` load the extension from `target/debug` (the `debug` entries of `godot/ofs.gdextension`); the `release` entries (`target/release`) only apply to an exported release build of the game, which this repository does not make yet. So run `cargo build -p ofs-godot` without `--release` and keep `CARGO_TARGET_DIR` at its default while running Godot — a scratch target dir (as the WSL firewall workaround below uses for Rust tests) leaves the client without its extension.
- **First import (fresh checkout):** a fresh `godot --headless --path godot --import` (no `godot/.godot/` yet) can segfault at process exit on Windows (exit code 139) after finishing its import work. Run it twice by hand, or let the runner do it; the second run is clean (warm imports are not affected).
- **Windows Firewall:** the game starts `ofs-sim.exe` itself. Windows may ask whether to allow it on the first run: allow private networks (the server only listens on `127.0.0.1`). With real Betaflight in WSL, a blocked `ofs-sim.exe` shows as "no motor output within 5000 ms" (see "Troubleshooting (Windows)" below).
- **Manual flight check** (needs a controller and, for step 3 onwards, real Betaflight with `OFS_SITL_LAUNCH` set):
  1. `godot --path godot`, press F2 and check the bindings of your radio or gamepad against the stick display (roll right, pitch forward, yaw right).
  2. Wait for the HUD to show a flying state, put the throttle low and arm with aux 1; switch aux 2 for Angle mode.
  3. Fly, then cut the radio (K, or switch the transmitter off): the HUD reports the link lost and Betaflight fails safe within a few seconds. R reloads the quad.
  4. Configurator: connect to the address the HUD shows (`tcp://127.0.0.1:5761`), change a value and Save (the Configuration tab's Save and Reboot reboots Betaflight; the HUD counts the restarts).
- **Visual check:** `godot --path godot -s res://tests/shots.gd -- --out=<dir>` opens a window briefly and saves screenshots of the start and a short hop in FPV and chase views plus the help and controls screens (`start_fpv.png`, `start_chase.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png`); review them by eye. Without `--out=` they go to the project's user data dir.

## Troubleshooting (Windows)
Live SITL tests run from Windows against SITL in WSL, and SITL's datagrams reach the Windows host through the Windows Firewall.
- **Symptom:** SITL looks silent and runs fail with "no motor output within 5000 ms" or a first-exchange timeout.
- **Cause:** a freshly built test executable, or `ofs-sim.exe`, is blocked by an inbound Block rule. A dismissed first-run firewall prompt creates one.
- **Fix, option 1:** allow the program in Windows Defender Firewall (private networks). This needs an administrator.
- **Fix, option 2:** run the Linux build natively inside WSL, which needs no firewall rule:
  ```
  cd /mnt/c/<repo path>
  CARGO_TARGET_DIR=$HOME/ofs/target cargo test --workspace --locked
  CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1
  ```
  WSL has no Python grpc module, so the Python tests need Windows.

## Sessions
- **Lockstep** (default): simulated time advances only through `Run`. Runs are deterministic, Betaflight included.
- **Real time:** `load(..., mode="realtime")`, then `start()`/`pause()`. A paused real-time session can still be single-stepped with `run()`.
  - Overruns are counted in `State.overruns`.
  - `overrun_policy="warn"` (default) drops a backlog over 100 ms.
  - `"slow"` lets simulated time stretch instead.
- **Lifetime:**
  - A Python client's session ends when the client disconnects, unless it was loaded with `keep_alive=True`.
  - A pilot client disconnecting (the `Pilot` stream) switches the radio transmitter off: Betaflight fails safe and the session keeps running.

## Firmware state
- **Directory:** each quad file gets `<data_dir>/<quad file stem>-<8 hex digits>/` (the digits hash the quad file's path), holding `eeprom.bin`, `betaflight.diff` (the copy applied on first boot), `sitl.log` and any Blackbox logs.
- **Diff changes:** the quad's diff is applied only on first boot, so Configurator changes persist. If the quad's diff changes later, loading fails until you delete `eeprom.bin` (which also discards Configurator changes).
- **Reboots:** a Betaflight reboot (Configurator "Save", MSP_REBOOT) relaunches SITL from its EEPROM; `State.fc_restarts` counts them.

## Betaflight Configurator
SITL serves MSP on `tcp://127.0.0.1:5761` (also from Windows, through WSL's localhost forwarding), but only while simulated time advances. So connect while a real-time session runs:
1. Start `python python/examples/serve_realtime.py` (with `OFS_SITL_LAUNCH` set). It prints the Configurator address.
2. In the Betaflight Configurator desktop app, enable manual connection in the options, enter `tcp://127.0.0.1:5761`, and press Connect.
3. Saving reboots Betaflight and the simulator relaunches it, so the Configurator should reconnect. The relaunch is verified live (an `MSP_REBOOT` sent to the Configurator port made the simulator relaunch SITL from its EEPROM in about 1.6 s, and the radio link came back up), but the Configurator application itself has not been tried yet: the manual check below is pending.

The web Betaflight App cannot open raw TCP connections.

To check that Configurator changes persist (this check has not been run yet), connect as above, then:
1. change one PID value and press Save;
2. reconnect and confirm the value persisted;
3. note `betaflight restarts=1` in the script's output.

## Ports
SITL uses fixed ports (UDP 9001–9004, TCP 5760+), so only one SITL session can run per machine and SITL tests must not run in parallel.
