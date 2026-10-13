# Developer setup

## Requirements
- Rust stable ≥ 1.85 (`rustup`), Python ≥ 3.10.
- Betaflight SITL: Linux, or WSL2 (Ubuntu) on Windows. Build with `bash scripts/build-sitl.sh` (see the script for env options). It applies `third_party/betaflight/ofs-sitl.patch`: lockstep time, UART bytes in the state datagram, deterministic boot.

## Everyday commands
| What | Command |
|---|---|
| All Rust tests | `cargo test --workspace` |
| Live SITL tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` |
| Live video tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-video --test vtx_live -- --ignored --test-threads=1`, and `python -m pytest python/tests/test_sitl_video.py` |
| Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` (Ctrl-C stops it and its SITL) |
| Python tests | `cargo build -p ofs-sim && python -m pytest python/tests -v` (after `python -m pip install -e "python[dev]"`, which brings the recording, Parquet and Rerun extras the tests use) |
| Live Python tests in WSL | from `python/`: `OFS_SIM_BIN=$HOME/ofs/target/debug/ofs-sim OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf PYTHONPATH=. uv run --no-project --with grpcio,protobuf,pytest,mcap,pandas,pyarrow,'rerun-sdk>=0.39,<0.40' python -m pytest tests` (a Linux-built `ofs-sim`) |
| Build the Python package | `python -m build python --outdir dist` (wheel and sdist; CI publishes them on `v*` tags only) |
| Godot client tests | `bash scripts/run-godot-tests.sh` (see "Godot pilot client" below) |
| Real-time session (for Configurator) | `OFS_SITL_LAUNCH=<cmd> python python/examples/serve_realtime.py` |
| Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |
| Lint (CI runs it) | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Regenerate the SITL patch | start from the M1 patch (`third_party/betaflight/ofs-sitl.patch` at commit c1514e2) applied to the pinned Betaflight checkout, run `add_serial_in_datagram.py` then `deterministic_boot.py` (both in `third_party/betaflight/tools/`; see their docstrings), then `add_serial_out_datagram.py` (M3a: UART TX bytes in the reply). Run the generators in that order. |

Commands written `VAR=value command` are for bash (Linux, WSL, Git Bash). In PowerShell set the variable first, for
the rest of the session: `$env:OFS_SITL_LAUNCH = "wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"`,
then run the command.

## Environment variables
- `OFS_SIM_BIN` — path to `ofs-sim` used by `ofs.launch()`.
- `OFS_SITL_LAUNCH` — SITL argv, space-separated (quote a path with spaces: `"..."` or `'...'`), overrides `fc.launch` in quad files.
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
- **Visual check:** `godot --path godot -s res://tests/shots.gd -- --out=<dir>` opens a window briefly and saves screenshots of the start and a short hop in FPV and chase views, the OSD, the three looks of the analog video link (`video_grain.png`, `video_unstable.png`, `video_lost.png`), and the help and controls screens (`start_fpv.png`, `start_chase.png`, `osd_fpv.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png`); review them by eye. Without `--out=` they go to the project's user data dir.

## OSD, VTX and battery telemetry (M3a)

Quad files are schema 5: the hardware lives in `library/` part files (see the "Component library" section of
README.md) and the quad keeps its own wiring — including three video sections (all optional; see
`quads/opendrone-5f-freestyle.toml`):
- `[esc_telemetry]` — UART 3. The simulated battery is encoded as KISS ESC-telemetry frames (the diff sets `battery_meter = ESC` and `current_meter = ESC`), so Betaflight's battery meter, OSD elements and warnings run on the simulated battery.
- `[osd]` — UART 4. Betaflight draws its OSD over MSP DisplayPort (`osd_displayport_device = MSP`); the simulator decodes the frames into a `cols` x `rows` character grid (30x16, the analog PAL grid).
- `[vtx]` — UART 5. The section references the VTX part (`part = "opendrone-smartaudio-1w"`, a SmartAudio v2.1 VTX) and carries this aircraft's wiring (the UART, default band, channel and power); the part answers Betaflight's requests, and the state carries its band, channel, frequency and power, with changes arriving as `vtx_changed` events.

The quad's Betaflight diff must configure the matching serial ports (UART3 = serial 2, UART4 = serial 3, UART5 = serial 4), the OSD element positions, `vcd_video_system = PAL` (AUTO would leave a 13-row NTSC grid) and `force_battery_cell_count` matching `battery.cells` (Betaflight otherwise guesses from the first voltage it sees, and a part-charged 6S pack never raises the low-battery warning); the shipped quads' diffs are the examples. SITL has no `vtxtable`: Betaflight takes the VTX power list from the VTX itself, so the diff has no `vtxtable` lines. A diff that enables SmartAudio (`serial 4 2048 …`) needs a matching `[vtx]` section — without one the quad fails to load, because Betaflight SITL crashes when its OSD draws the VTX channel and no VTX answers. The diff is applied on first boot only: after changing it, delete the firmware directory's `eeprom.bin` (see "Firmware state" below).

Wiring: SITL buffers the bytes it writes to its UARTs (UART1, the MSP/Configurator port, is excluded) in 4096-byte per-UART capture buffers and returns them in the reply datagram — a dropped count, then `[uart index][len][bytes]` blocks, at most 512 bytes per reply — so OSD and SmartAudio traffic is deterministic in lockstep. Bytes that don't fit are dropped and counted (`State.serial_dropped_bytes`); a rise raises `serial_overflow` events (see `docs/research/sitl-interface.md` §9).

From Python: `s.get_osd().text` is the OSD as text (`present` is False without an OSD; Betaflight's symbols show as `?`), and `state.vtx` carries the VTX's band, channel, frequency and power; `stream_osd()` yields frames as they change.

The OSD is drawn with Betaflight's own font: `tools/make_osd_font.py` converts the Configurator's `default.mcm` into the glyph atlas `godot/ui/osd_font.png`; `godot/ui/OSD_FONT_LICENSE.md` records the source commit and the licence.

Manual video check (needs a controller, real Betaflight with `OFS_SITL_LAUNCH` set, and the game):
1. Fly with the game and see the OSD battery, flight time and craft name in the FPV view.
2. Open the OSD stick menu (throttle centre + yaw left + pitch forward, disarmed).
3. Change the VTX channel or power in the OSD menu and watch the HUD VTX line and a toast.
4. Change the same from Betaflight Configurator's VTX tab and Save.
5. Lower `battery.initial_soc` to 0.02 and see LOW BATTERY.

## The world file and the analog video link (M3b)

A session flies in a world: `LoadRequest.world_path` (Python `load(..., world=...)`, the game's `world_path` setting,
`OFS_WORLD`, `--world=`) names a world file, and without one the server uses a built-in open field (the pilot at home,
one omni, nothing else). The game loads `worlds/flat.toml` by default.

A world file (`schema_version = 1`, NED metres from home like the quad file, sizes `[north, east, height]`) has:
- `[pilot]` — where the goggles are and which way the pilot faces;
- `[receiver]` — the noise floor, diversity, and the goggle antennas (`omni` or `patch` with a beamwidth; gain,
  polarization, aim relative to the pilot's facing);
- `[[objects]]` — boxes and vertical cylinders with a colour and an `rf_loss_db` (0: transparent to the signal).
  The game draws them from the world the server loaded (`GetWorld`), so what you see is what the video link sees.
  Nothing collides with the drone (the simulator's ground is the plane d = 0);
- `[[emitters]]` — other transmitters (a frequency, or a band and channel, and a power), which interfere with
  neighbouring channels. The shipped world has one, on R2, next to the quad's default R1.

The VTX part carries the antenna (`[vtx.antenna]`: kind, gain, polarization, `mount_frd`) and the pit-mode
power (`pit_power_mw`); the quad's `[vtx]` references the part and keeps the wiring. The VTX transmits from
load on, also in open loop (it has no Betaflight to answer
then, so it stays on its power-up channel and power).

The link model runs once per PAL field (50 Hz): path loss, antenna patterns, polarization, the quad's own frame,
diffraction around objects, the ground bounce, fading and interference give each goggle antenna's signal; diversity
picks one; the SNR decides the picture (grain, sparkles, colour, tearing, rolling, static). The equations and every
constant are in `docs/research/video-link.md`. The results are in `State.video` (Python `state.video`), and
`video_lost` / `video_restored` events mark sync losses. In the game, the HUD's `VID` line shows the SNR, the antenna
in use and its signal, and the layer `godot/ui/video.gd` draws the breakup over the picture and Betaflight's OSD. The
**Video effects** box on the F2 screen (also `OFS_VIDEO_EFFECTS`, `--video-effects=`) turns the drawing off; it is
saved in `user://ofs_client.cfg`. The **Picture** button beside it flips the FPV box between 4:3 (the default),
16:9 and window-fill (`fpv_aspect`; `OFS_FPV_ASPECT`, `--fpv-aspect=`), saved the same way.

Manual check (with the game, real Betaflight optional):
1. Fly away from the pilot (the figure beside the launch pad) and watch grain, then sparkles, colour loss, tearing and
   static as the HUD's `VID` number drops; come back and the picture relocks.
2. Fly behind building B (the tall one, east of the course): the picture breaks up within a few metres.
3. With Betaflight: lower the VTX power in the OSD menu (25 mW) and the picture breaks up much closer; pit mode leaves
   a picture only next to the pilot.
4. Switch the VTX to R2 (the parked quad's channel) and the picture gets noisier; any channel far from R2 is clean.
5. Untick Video effects on the F2 screen: the picture is clean whatever the link does.

## World collision and the ExpressLRS link (M3c)

Quad files are **schema 5**: since M4a the hardware lives in `library/` part files that the quad references by id
(see the "Component library" section of README.md), and a schema-4 file is refused at load with a message naming
the move. Schema 4's own change survives: the fixed link figures (`radio.rssi_dbm`, `radio.snr_db` and the loss
parameters `loss_good`, `loss_bad`, `p_good_to_bad`, `p_bad_to_good`) are gone — RSSI, SNR and packet loss come
from the world's geometry now — and `radio.tx_power_mw`, the `[[radio.antennas]]` list and the collision data
(the frame part's optional `[collision]` table since schema 5) replace them. `quads/opendrone-5f-freestyle.toml`
is the example. The world file gains an optional `[handset]` (where the pilot's transmitter is and what it transmits
with; the default sits 0.5 m below the goggles with one vertical 2 dBi dipole). The contact model and every constant
are in `docs/research/collision.md` and `docs/research/elrs-link.md`.

### M3c manual checks

1. Hit a wall: fly at BuildingA in the Flat field — the HUD toasts `HIT BuildingA ... m/s` and the quad bounces.
2. Clip a gate post: brush Gate0PostL at speed — the quad tumbles, one toast.
3. Land on a roof: settle on BuildingC — the quad rests, no jitter, no toast below 1 m/s.
4. Watch the link: with a quad file at `tx_power_mw = 10`, fly low behind BuildingB — the HUD's SNR falls,
   LQ decays, and Betaflight fails safe when the window drains.

## Flight-controller telemetry (M3d)

Updating the repository requires rebuilding SITL (`bash scripts/build-sitl.sh` again after a pull): the patch grows
with the milestones, and a stale binary refuses what it was never taught. Since M3d a stale binary prints one
`[SITL] 8 serial bytes for UART251 dropped: no such open UART` line per datagram — the simulator's per-motor eRPM
block, which the old `sitl.c` does not intercept — and flies without motor telemetry.

What is new: Betaflight's own CRSF telemetry rides the ELRS downlink (the model is in
`docs/research/elrs-link.md`, "Downlink telemetry"), so the HUD's battery line shows Betaflight's own voltage,
current and flight mode while the telemetry is fresh and dashes out when it is not; and the simulator's per-motor
eRPMs reach Betaflight's RPM filter. No configuration was involved — schema-4 quad files, unchanged then, are
refused since M4a's schema 5, and pole pairs are now configuration, validated against the diff.

Manual check (needs a controller, real Betaflight with `OFS_SITL_LAUNCH` set, and the game):
1. Fly and punch the throttle: the HUD battery line (`12.40 V   3.2 A   ACRO` — Betaflight's values, the flight
   mode appended) sags with the punch-outs.
2. Cut the link (K, or switch the transmitter off): the battery line dashes out while the picture and the rest of
   the HUD stay.
3. Open the Configurator's Motors tab (connect as in "Betaflight Configurator" below): four distinct per-motor
   RPMs track the throttle.

## Video timing, the DVR and the Tramp VTX (M3e)

The client's picture is a delayed composite: the world file's `[receiver]` section gains `latency_ms` (the
goggles' glass-to-glass latency; default 40 ms = 2 PAL fields, quantized down to whole fields, at most 1000),
and the client samples pose, video quality and OSD at one field-quantized instant, `sim_now − latency` — the
picture lags the world and judders at the field rate, like real analog glass. In the marginal SNR band (between
6 dB and 3 dB) the receiver fails to complete fields — a dropped field holds the previous picture (pose, OSD
and quality all freeze) for one field. The model and its constants are in `docs/research/video-link.md`.

The **DVR** (keybind **D**) records the FPV view — exactly what the pilot sees — as an MJPEG AVI into
`user://recordings/` (`%APPDATA%\Godot\app_userdata\Open FPV Sim\recordings\` on Windows,
`~/.local/share/godot/app_userdata/Open FPV Sim/recordings/` on Linux). It captures on field boundaries; a
dropped field repeats the held frame, so the recording freezes where the pilot froze. Pressing D again stops
it, and it stops on its own (with a toast counting the saved frames) when the picture's INTERNAL size changes:
an aspect switch or a chase-camera toggle, and — because the window aspect renders at the window's own size —
a plain window resize with the window aspect. With a boxed aspect the internal resolution is fixed (960×720 or
1280×720), so a plain window resize does not stop it. It
records only the FPV view: press C first if the chase camera is up.

The **Tramp quad** `quads/opendrone-5f-tramp.toml` is the shipped freestyle quad with a Tramp VTX instead of
the SmartAudio one: `[vtx] kind = "tramp"` on UART6 (Betaflight serial 5, function mask 8192 = `VTX_TRAMP`).
No SITL changes were needed (the patch stopped `#undef`-ing `USE_VTX_TRAMP` in M3a), and SITL has no
`vtxtable`, so Betaflight uses the driver's built-in 25/100/200/400/600 mW power table. Tramp is
frequency-only: band and channel settings convert to nothing (Betaflight's `vtxTrampSetBandAndChannel` is a
no-op), and the frequency comes from the VTX's own power-up state (R1, 200 mW); what moves it is a frequency
set, which the model answers like the SmartAudio one. Pit mode goes through Tramp's 'I' command (0 = pit on).
Fly it from the game with real Betaflight:

    godot --path godot -- --quad=quads/opendrone-5f-tramp.toml

The live test behind it is `OFS_SITL_LAUNCH=<cmd> python -m pytest python/tests/test_sitl_tramp.py` (the CI
sitl job runs it with the other live Python tests).

M3e manual check (needs a controller and, for the Tramp check, real Betaflight with `OFS_SITL_LAUNCH` set):
1. Fly with the default latency: the picture trails the sticks and judders at the field rate — visible but
   flyable — and freezes for a field here and there near the edge of the range.
2. Record a clip with D while flying the link through grain and static, press D again, and play the `.avi`
   from `user://recordings/` in VLC: the breakup, the OSD and the freezes are all in the file.
3. Flip the Picture setting through 4:3, 16:9 and window on the F2 screen: the box changes shape, and a
   recording running across the switch stops with a toast.
4. On the Tramp quad, open the OSD stick menu (as in the M3a check) and change the VTX power: the HUD VTX line
   snaps to the built-in table. A band/channel change does nothing — Tramp is frequency-only; the live test
   pins the frequency path.

## The fault catalog (M4b)

The client and server speak **protocol 8**: the faults' wire shapes live in the `Fault` oneof of
`proto/ofs/v1/sim.proto`, the injectable Python dataclasses in `python/ofs/faults.py` (`sim.inject(...)` /
`sim.clear_faults()`), and the models, constants and their estimate status in `docs/research/faults.md` —
regenerate the stubs with the table's protoc recipe after changing the proto.

## Recording, scripted inputs and the package (M4c)

The client and server speak **protocol 9**: `StartRecording`/`StopRecording`, `PlayInputs`/`StopInputs`, a fault's
optional `at_s`, world names in `LoadRequest.world_path`, and `HandshakeReply.logs_dir`.

- **Recordings** land in `<data_dir>/logs/<quad stem>-<n>.mcap`; the server names them and never writes a
  client-supplied path. The recorder (`crates/ofs-log`) is the last model of each tick and only reads the bus. A
  record carries the state `GetState` reports at its time (`{"v": x}`, `{"x", "y", "z"}` or `{"w", "x", "y",
  "z"}` JSON with a JSON schema); the default rates are `ofs_log::DEFAULT_RATES`. The files are deterministic:
  the `mcap` crate is pinned to an upstream revision that writes its summary in id order
  (`docs/research/mcap-determinism.md`), and the writer pins its library string and single-threaded zstd. It keeps
  the per-message index records: Foxglove refuses a file without them.
- **Python:** a lockstep `load` starts a recording (`record=False` does not), `sim.log` stops and returns it,
  `ofs.reader` reads any recording (`mcap` + `pandas`; Parquet also `pyarrow`), `ofs.rerun.view(sim)` is the live
  view (`rerun-sdk` 0.39.x, pinned by the extra).
- **Scripted inputs** (`sim.play_inputs(path)`): the client sends the CSV's text (the server accepts requests up
  to 64 MiB, `MAX_REQUEST_BYTES`, so a long flight's file fits); the server validates it once
  (`crates/ofs-sim/src/inputs.rs`) and applies each row on the first tick at or after its time. Faults with `at=`
  use the same tick rule. Both apply right after the step that reaches their time, so a `run` that ends there
  already shows them.
- **Worlds by name:** `load(world="flat")` reads `worlds/flat.toml` beside the quad's directory, as parts resolve
  against `library/`.
- **Blackbox:** the shipped diffs set `blackbox_device = VIRTUAL`; SITL writes `LOGnnnnn.BFL` into the quad's
  firmware directory, and the session copies the logs it wrote into the logs directory when it ends.
- **Packaging:** the PyPI distribution is `open-fpv-sim` (import `ofs`), pure Python; `launch()` still needs an
  `ofs-sim` binary. The `publish` CI job runs on `v*` tags only, checks the tag equals `python/pyproject.toml`'s
  version and uploads through PyPI trusted publishing (the `pypi` environment); no token is stored.

## Troubleshooting (Windows)
Live SITL tests run from Windows against SITL in WSL, and SITL's datagrams reach the Windows host through the Windows Firewall.
- **Symptom:** SITL looks silent and runs fail with "no motor output within 5000 ms" or a first-exchange timeout.
- **Cause:** a freshly built test executable, or `ofs-sim.exe`, is blocked by an inbound Block rule. A dismissed first-run firewall prompt creates one.
- **Fix, option 1:** allow the program in Windows Defender Firewall (private networks). This needs an administrator.
- **Fix, option 2:** run the Linux build natively inside WSL, which needs no firewall rule:
  ```
  cd /mnt/c/<repo path>
  CARGO_TARGET_DIR=$HOME/ofs/target cargo test --workspace --locked
  CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf cargo test --workspace --locked -- --include-ignored --test-threads=1
  ```
  The second command runs every Rust test, the live ones (`sitl_live`, `vtx_live`) included.
  WSL's system Python has no grpc module: run the Python tests there through `uv` (the "Live Python tests in
  WSL" command above), or from Windows.

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
- **Diff changes:** the quad's diff is applied only on first boot, so Configurator changes persist. If the quad's diff changes later, loading fails until you delete `eeprom.bin` (which also discards Configurator changes). M4c changed both shipped diffs (`set blackbox_device = VIRTUAL`): delete the `eeprom.bin` of each quad you flew before it, once.
- **Reboots:** a Betaflight reboot (Configurator "Save", MSP_REBOOT) relaunches SITL from its EEPROM; `State.fc_restarts` counts them.

## Betaflight Configurator
SITL serves MSP on `tcp://127.0.0.1:5761` (also from Windows, through WSL's localhost forwarding), but only while simulated time advances. So connect while a real-time session runs:
1. Start `python python/examples/serve_realtime.py` (with `OFS_SITL_LAUNCH` set). It prints the Configurator address.
2. In the Betaflight Configurator desktop app, enable manual connection in the options, enter `tcp://127.0.0.1:5761`, and press Connect.
3. Saving reboots Betaflight and the simulator relaunches it, so the Configurator should reconnect. The relaunch is verified live (an `MSP_REBOOT` sent to the Configurator port made the simulator relaunch SITL from its EEPROM in about 1.6 s, and the radio link came back up), but the Configurator application itself has not been tried yet: the manual check below is pending.

The web Betaflight App cannot open raw TCP connections.

Persistence over MSP is covered live by `test_a_pid_saved_over_msp_persists_through_a_reboot_and_a_new_session`
(`python/tests/test_sitl_video.py`). To check it with the Configurator application itself, connect as above, then:
1. change one PID value and press Save;
2. reconnect and confirm the value persisted;
3. note `betaflight restarts=1` in the script's output.

## Performance
`scripts/benchmark.py` measures the real-time factor (simulated seconds per wall second) of headless lockstep runs
of the shipped quad: open loop and with Betaflight SITL, recording on and off. The parent spec's target (§8.6) is
at least 2× on a mid-range PC; CI's sitl job runs it against a release build, prints the table in the job summary
and fails only below 1× (shared runners are noisy).

Measured on an AMD Ryzen 9 9900X3D, release build, 2026-10-13. Windows, open loop (no Betaflight):

| Configuration | Simulated (s) | Wall (s) | Real-time factor |
|---|---|---|---|
| open loop, recording | 30.0 | 0.31 | 97.8x |
| open loop, no recording | 30.0 | 0.10 | 295.7x |

WSL 2 on the same machine, with Betaflight SITL:

| Configuration | Simulated (s) | Wall (s) | Real-time factor |
|---|---|---|---|
| open loop, recording | 30.0 | 0.36 | 84.3x |
| open loop, no recording | 30.0 | 0.14 | 208.0x |
| Betaflight SITL, recording | 30.0 | 4.08 | 7.4x |
| Betaflight SITL, no recording | 30.0 | 3.82 | 7.9x |

The lockstep exchange with SITL is the cost: every configuration clears the 2× target, the Betaflight ones by
almost four times.
On CI's shared Linux runner (native, no WSL; run 38453432260) the Betaflight rows measured 12.4× and 13.8×.

## Ports
SITL uses fixed ports (UDP 9001–9004, TCP 5760+), so only one SITL session can run per machine and SITL tests must not run in parallel.
