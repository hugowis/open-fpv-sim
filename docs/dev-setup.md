# Developer setup

## Requirements
- Rust stable ≥ 1.85 (`rustup`), Python ≥ 3.10.
- Betaflight SITL: Linux, or WSL2 (Ubuntu) on Windows. Build with `bash scripts/build-sitl.sh` (see the script for env options). It applies `third_party/betaflight/ofs-sitl.patch`: lockstep time, UART bytes in the state datagram, deterministic boot.
- Windows without WSL (experimental): build the SITL natively with MSYS2, see "Native Windows SITL" below.
- macOS (Apple silicon, best effort): everything builds and runs natively, SITL included, see "macOS" below.

## Everyday commands
| What | Command |
|---|---|
| All Rust tests | `cargo test --workspace` |
| Live SITL tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` |
| Live video tests | `OFS_SITL_LAUNCH=<cmd> cargo test -p ofs-video --test vtx_live -- --ignored --test-threads=1`, and `python -m pytest python/tests/test_sitl_video.py` |
| Server | `cargo run -p ofs-sim -- --listen 127.0.0.1:50051 --data-dir .ofs-data` (Ctrl-C stops it and its SITL; the game does not need the port free — `--server 127.0.0.1:0` has its launcher take whatever the OS picks, read back from the server) |
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
- `OFS_SITL_CLEANUP` — argv run before launch and after stop to kill stray SITL processes (also when a native SITL's UDP 9003 is still held). It defaults to removing the stray SITL of the session's own slot (a small `sh` script that SIGTERMs the processes named `betaflight_SITL` launched with the slot's `--port-offset`; see "Ports and parallel sessions") on Linux, and the same inside the VM with the `<wsl prefix>` under WSL (required there: a stale SITL would otherwise answer instead of the new one). It matches the process name, never `pkill -f` on the command line, which also matches the shell running it. A command of your own, such as `pkill -x betaflight_SITL`, kills every SITL by name.
- `OFS_SITL_HOST`, `OFS_SITL_REPLY_IP` — override the address state datagrams go to, and the address SITL replies to (`--ip`, motor socket bind).

## Godot pilot client
The game client lives in `godot/`; README.md describes what it is, its keys and its settings. It needs Godot **4.7.2** and the extension plus server built:

    cargo build -p ofs-sim -p ofs-godot

- **Install Godot:** download the official release zip (`Godot_v4.7.2-stable_win64.exe.zip`; Linux: `Godot_v4.7.2-stable_linux.x86_64.zip`) from the 4.7.2-stable release page and check it against that release's `SHA512-SUMS.txt`. For headless runs on Windows use the console binary from the same zip (`Godot_v4.7.2-stable_win64_console.exe`): the standard exe is a GUI program that detaches from the terminal, so its output is lost.
- **Tests:** `bash scripts/run-godot-tests.sh [unit|open-loop|errors|terrain|betaflight|e2e|all]` runs the unit suites and the end-to-end tests (the `terrain` suite among them). It downloads Godot 4.7.2 itself (SHA-512 checked, into `build/godot-dl/`, override with `OFS_GODOT_DL`) unless `GODOT_BIN` points at any 4.7 binary, and it always builds the server and extension first (a no-op when up to date). The Betaflight e2e runs only when `OFS_SITL_LAUNCH` is set.
- **Direct commands** (what the script wraps; both e2e scripts fly a real ofs-sim):
  - `godot --headless --path godot -s res://tests/run_tests.gd` — the unit suites;
  - `godot --headless --path godot -s res://tests/e2e_open_loop.gd` — open-loop e2e (no firmware);
  - `godot --headless --path godot -s res://tests/e2e_terrain.gd` — the `terrain` suite: the mesh the client draws against the sim's `terrain_height` over `worlds/hills.toml`;
  - `OFS_SITL_LAUNCH=<cmd> godot --headless --path godot -s res://tests/e2e_betaflight.gd` — e2e with real Betaflight (armed through CRSF, climbs, reload re-arms).
- **`timeout -k`:** a test script with a parse error exits 1, but a script that raises a runtime error never exits — the engine just keeps running. The runner therefore wraps every Godot call in `timeout -k` and treats a kill (rc 124) as a failure.
- **`CARGO_TARGET_DIR`:** the Godot editor and `godot --path godot` load the extension from `target/debug` (the `debug` entries of `godot/ofs.gdextension`); the `release` entries (`target/release`) only apply to an exported release build of the game, which this repository does not make yet. So run `cargo build -p ofs-godot` without `--release` and keep `CARGO_TARGET_DIR` at its default while running Godot — a scratch target dir (as the WSL firewall workaround below uses for Rust tests) leaves the client without its extension.
- **First import (fresh checkout):** a fresh `godot --headless --path godot --import` (no `godot/.godot/` yet) can segfault at process exit on Windows (exit code 139) after finishing its import work. Run it twice by hand, or let the runner do it; the second run is clean (warm imports are not affected).
- **Windows Firewall:** the game starts `ofs-sim.exe` itself. Windows may ask whether to allow it on the first run: allow private networks (the server only listens on `127.0.0.1`). With real Betaflight in WSL, a blocked `ofs-sim.exe` shows as "no motor output within 5000 ms" (see "Troubleshooting (Windows)" below).
- **Manual flight check** (needs a controller and, for step 3 onwards, real Betaflight with `OFS_SITL_LAUNCH` set):
  1. `godot --path godot` (the game opens in the menu: Start flying), press F2 and check the bindings of your radio or gamepad against the stick display (roll right, pitch forward, yaw right).
  2. Wait for the HUD to show a flying state, put the throttle low and arm with aux 1; switch aux 2 for Angle mode.
  3. Fly, then cut the radio (K, or switch the transmitter off): the HUD reports the link lost and Betaflight fails safe within a few seconds. R reloads the quad.
  4. Configurator: connect to the address the HUD shows (`tcp://127.0.0.1:5761` on an otherwise idle machine — it is the slot's, +10 per simulator, see the ports table), change a value and Save (the Configuration tab's Save and Reboot reboots Betaflight; the HUD counts the restarts).
- **Flight audio:** headless Godot has no audio device (the dummy driver still takes frames), so the Godot tests cover the pump and the `audio` setting, and the Rust tests (`cargo test -p ofs-audio`) cover the sound itself. Check it by ear on a machine with speakers: the motor pitch follows the throttle, the wind noise rises with speed, a crash makes a thud, and turning **Sound** off in Settings silences it (and the choice persists in `user://ofs_client.cfg`).
- **Visual check:** `godot --path godot -s res://tests/shots.gd -- --out=<dir>` opens a window briefly and saves screenshots of the start and a short hop in FPV and chase views, the OSD, the three looks of the analog video link (`video_grain.png`, `video_unstable.png`, `video_lost.png`), and the help and controls screens (`start_fpv.png`, `start_chase.png`, `osd_fpv.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png`, `menu.png`, `settings.png`, `terrain.png`, a hilly world seen from above); review them by eye. Without `--out=` they go to the project's user data dir. For terrain, a human looks at what the tests cannot judge: the relief reads as ground in `terrain.png`, gates stand on it, the quad lands on the ground it sees (fly `worlds/hills.toml` and `worlds/park.toml`), and the frame rate holds with the large mesh (hills is 361k vertices) in FPV and in the DVR view.

## Controller setup (#127)

The F2 screen (or **Controls** in the main menu, where **Esc** or the B button comes back) holds everything about the handset. It
persists to `user://controls.json`; a file from before profiles existed loads as the profile `Default`.

- **Profiles.** The picker switches the whole setup (preset, bindings, deadzone, shaping); **New** makes one from the name
  field (else the first controller's name), starting from the preset that name suggests (Xbox, PlayStation, 8BitDo...
  -> gamepad; RadioMaster, Taranis, Jumper, EdgeTX... -> radio; anything else -> radio); **Delete** removes the active one
  (the last cannot go).
- **Calibrate...** (needs a controller connected). Leave everything alone for a second (the centres), then move every
  stick to all its extremes and flip each switch, then **Done**. An axis that moved less than half its range is left as it
  was. The result belongs to the controller (its GUID), not to a profile, so it applies to every profile that reads that
  controller. A radio axis that reads -0.8..+0.9 then reaches exactly -1 and +1.
- **Deadband and travel** (roll, pitch, yaw). Deadband is the dead zone around the centre, rescaled so the output still
  reaches 1 (the **Deadzone** slider is the default for sticks without their own). Travel is the fraction of the throw that
  gives full output: 0.8 on a gamepad whose sticks stop short. Order: calibrate, invert, deadband, travel. Rates and expo
  are Betaflight's and are not offered here.
- **Monitor.** The stick boxes and aux squares are what the client sends; the bars are per channel; the line under them lists the
  controllers Godot sees.

Manual check with real hardware (headless Godot has no joystick): plug in a radio in joystick mode and a gamepad; check
the name shown, run Calibrate on each and confirm the bars reach both ends; create a profile per device and switch between
them; unplug and replug (the calibration should still apply); check the preset a new profile picks.

## OSD, VTX and battery telemetry (M3a)

Quad files are schema 6: the hardware lives in `library/` part files (see the "Component library" section of
README.md) and the quad keeps its own wiring — including three video sections (all optional; see
`quads/opendrone-5f-freestyle.toml`):
- `[esc_telemetry]` — UART 3. The simulated battery is encoded as KISS ESC-telemetry frames (the diff sets `battery_meter = ESC` and `current_meter = ESC`), so Betaflight's battery meter, OSD elements and warnings run on the simulated battery.
- `[osd]` — UART 4. Betaflight draws its OSD over MSP DisplayPort (`osd_displayport_device = MSP`); the simulator decodes the frames into a `cols` x `rows` character grid (30x16, the analog PAL grid).
- `[vtx]` — UART 5. The section references the VTX part (`part = "opendrone-smartaudio-1w"`, a SmartAudio v2.1 VTX) and carries this aircraft's wiring (the UART, default band, channel and power); the part answers Betaflight's requests, and the state carries its band, channel, frequency and power, with changes arriving as `vtx_changed` events.

The quad's Betaflight diff must configure the matching serial ports (UART3 = serial 2, UART4 = serial 3, UART5 = serial 4), the OSD element positions, `vcd_video_system = PAL` (AUTO would leave a 13-row NTSC grid) and `force_battery_cell_count` matching `battery.cells` (Betaflight otherwise guesses from the first voltage it sees, and a part-charged 6S pack never raises the low-battery warning); the shipped quads' diffs are the examples. SITL has no `vtxtable`: Betaflight takes the VTX power list from the VTX itself, so the diff has no `vtxtable` lines. A diff that enables SmartAudio (`serial 4 2048 …`) needs a matching `[vtx]` section — without one the quad fails to load, because Betaflight SITL crashes when its OSD draws the VTX channel and no VTX answers. The diff is applied on first boot only: after changing it, delete the firmware directory's `eeprom.bin` (see "Firmware state" below).

Wiring: SITL buffers the bytes it writes to its UARTs (UART1, the MSP/Configurator port, is excluded) in 4096-byte per-UART capture buffers and returns them in the reply datagram — a dropped count, then `[uart index][len][bytes]` blocks, at most 512 bytes per reply — so OSD and SmartAudio traffic is deterministic in lockstep. Bytes that don't fit are dropped and counted (`State.serial_dropped_bytes`); a rise raises `serial_overflow` events (see `docs/research/sitl-interface.md` §9).

From Python: `s.get_osd().text` is the OSD as text (`present` is False without an OSD; Betaflight's symbols show as `?`), and `state.vtx` carries the VTX's band, channel, frequency and power; `stream_osd()` yields frames as they change.

The OSD is drawn with Betaflight's own font: `tools/make_osd_font.py` converts the Configurator's `default.mcm` into the glyph atlas `godot/ui/osd_font.png`; `godot/ui/OSD_FONT_LICENSE.md` records the source commit and the licence. Betaflight sends a cell's severity as its font page: page 1 (INFO) is the atlas's bold face, and warnings (page 2) and criticals (page 3) are drawn from the bold glyphs tinted amber and red.

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
  The quad collides with them (`docs/research/collision.md`);
- `[[emitters]]` — other transmitters (a frequency, or a band and channel, and a power), which interfere with
  neighbouring channels. The shipped world has one, on R2, next to the quad's default R1.
- `[terrain]` — an opt-in height grid for the ground: `seed`, `size_m` (the square of relief, centred on home, flat beyond
  it), `cell_m`, `amplitude_m`, `wavelength_m`, `octaves`, `persistence` and `flat_radius_m` (exactly flat, d = 0,
  around home), every key optional. Without it the ground is the plane d = 0. The physics follows the surface and the
  Godot client draws the same grid (it comes in the `GetWorld` reply, `World.terrain`, protocol 16; the client checks its
  digest and builds the mesh itself); the radio and video link are obstructed by the relief (#154), and objects with
  `rf_loss_db` stand on it. `worlds/hills.toml` is the big example (12 m hills); `worlds/park.toml` has gentle swells of
  about a metre with its course placed on the ground. The steepest slope may not pass 60 degrees.
- `[[meshes]]` and `[[materials]]` — triangle meshes from `.glb` files, solid to the quad and in the way of the radio.
  A mesh has a `name` (letters, digits, `_` and `-`, unique), a `file` (relative to the world file, ending `.glb`, no `..`,
  no absolute path and no `:`), a `position_ned_m` and a `heading_deg` (clockwise from north; the file's origin goes to
  the position). A material is the glTF material's name (any non-empty, unique string, matched exactly; an unnamed
  one is called `default`) with an `rf_loss_db` (0 if it is not listed; a listed material that no mesh uses is an error).
  `worlds/hall.toml` flies a concrete hall with a doorway and a glass window (`worlds/assets/hall.glb`, made by
  `cargo run -p ofs-config --example make_hall`). Caps (estimates): 8 meshes, 400 000 triangles in all and 64 MiB per file.
  - **The file** is in Godot's frame (x east, y up, -z north), so NED = (-z, x, -y). Only triangle lists are read (other
    primitive modes are skipped); sparse accessors, external buffers and a node graph that is not a tree are refused, and
    zero-area triangles are dropped. A node whose own name starts with `col_` is collision and radio only and Godot does not
    draw it (children do not inherit the prefix); if the file has no `col_` node, the visible nodes are used for both.
  - **Physics**: a sphere is tested against the *surface*, two-sided, so a mesh need not be watertight; the landing points
    use only the nearest triangle, with a 5 mm skin. A hit is `HIT <mesh name>`. The maths and limits are in
    `docs/research/collision.md`.
  - **Radio**: a path loses the `rf_loss_db` of every surface it crosses, summed, at any frequency. There is no diffraction
    around a mesh's edges (the shadow is sharp), no reflection, and no thickness: a wall drawn as two skins costs two
    losses, a wall that is one surface costs one, a window is a surface with a small loss and an opening passes the signal.
  - **The client** loads the same `.glb` itself: the protocol carries only the absolute path and an FNV-1a 64 digest
    (`World.meshes`, protocol 17), so the simulator and the Godot client must share a filesystem. A client whose file differs
    draws nothing for that mesh and `OfsClient.get_world()["meshes"]` says why. A repro bundle carries the mesh files under
    `worlds/`. A world without `[[meshes]]` is bit-identical to before; a mesh with a non-zero `heading_deg` is turned with
    libm `sin` and `cos` at load, so only `heading_deg = 0` meshes are claimed bit-identical across operating systems. The
    Godot drawing has been verified headless only.

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

Quad files are **schema 6**: since M4a the hardware lives in `library/` part files that the quad references by id
(see the "Component library" section of README.md; the IMU and barometer joined at schema 6, and schema-5 inline `[imu]`/`[baro]` tables still load; schema 7 adds the optional `mag = "id"` magnetometer, the shipped quads stay at 6), and a schema-4 file is refused at load with a message naming
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

The **DVR** (keybind **D**, or its checkbox on the F2 controls screen — the same switch) records the FPV view as an
MJPEG AVI into `user://recordings/` (`%APPDATA%\Godot\app_userdata\Open FPV Sim\recordings\` on Windows,
`~/.local/share/godot/app_userdata/Open FPV Sim/recordings/` on Linux). It captures on field boundaries; a dropped
field repeats the held frame, so the recording freezes where the pilot froze. Pressing D again (or unchecking the
box) stops it. A window resize or an aspect switch does not stop it: frames are scaled to fit inside the size the
recording started with and centred on black bars. Entering the chase view (C) stops it, and so does a reload or a
lost session, each with a toast counting the saved frames. It records only the FPV view: press C first if the chase
camera is up. The checkbox always shows the state that actually happened: D checks and unchecks it, and so does an
automatic stop; a refused start (the chase camera up, or no simulator) leaves it unchecked.

Two settings on the F2 screen (each a button that cycles its values) choose what is recorded and what happens
afterwards; they follow the usual order (defaults < `ofs_client.cfg` < environment < flags), and an invalid value
falls back to the default:

| Setting | F2 button | Values (default first) | Environment | Flag |
|---|---|---|---|---|
| `dvr_feed` | `DVR feed: ...` | `goggles`, `clean`, `both` | `OFS_DVR_FEED` | `--dvr-feed=` |
| `dvr_export` | `DVR export: ...` | `none`, `frames`, `mp4`, `both` | `OFS_DVR_EXPORT` | `--dvr-export=` |

- **Feed.** `goggles` is the picture the pilot sees (analog noise, OSD, receiver artefacts): `REC_<ts>.avi`. `clean`
  is the camera image alone, without the video-link effects and the OSD: `REC_<ts>_clean.avi`. `both` writes the two
  files side by side. The feed applies to the next recording. Both feeds write on every field, so their files keep
  equal frame counts unless the stop toast reports dropped frames (each file has its own writer queue); a feed that
  has no picture yet writes black, then repeats its last frame. The clean viewport renders only while a clean
  recording runs and has no picture of the new recording on its first field, so that first clean frame is black. The
  headless tests feed synthetic images instead of the viewports.
- **Export.** After any stop that saved frames (D, the F2 box, entering the chase view, a reload or a lost session),
  each file is exported as the setting asks; an abnormal stop and a start that failed export nothing. `frames` copies the AVI's JPEG frames unchanged to
  `<name>_frames/frame_000001.jpg`, `frame_000002.jpg`, ... (the directory must not already exist; it also works on
  an AVI that was never finalised by a crash). `mp4` runs `ffmpeg` from `PATH`; the ffmpeg must be built with
  libx264 and an AAC encoder (its native `aac`). With the recording's audio (below) the encode is `ffmpeg -nostdin -n
  -loglevel error -i <name>.avi -f s16le -ar 44100 -ac 1 -i <pcm> -vf pad=ceil(iw/2)*2:ceil(ih/2)*2 -c:v libx264
  -pix_fmt yuv420p -crf 18 -movflags +faststart -c:a aac -b:a 128k -shortest <name>.mp4`: H.264 with an even size,
  quality 18, the index at the front for streaming, and AAC at 128 kb/s, mono. Without it the audio input and
  options are left out and the MP4 is silent. If ffmpeg is missing, a warning toast says so and the AVI is kept. An
  export never overwrites a file or directory; it runs on a worker thread and toasts when it is done. The AVI is
  always kept.
- **Audio.** While a feed records, the client writes `<name>.audio` beside the AVI (`REC_<ts>.audio`,
  `REC_<ts>_clean.audio`): the header `ofs-dvr-audio 1`, then an `F` line per frame the writer accepted (`F <state_time_s> <collision_speed_mps> <airspeed_mps> <descent_mps> <n> <rpm_1..rpm_n> <cmd_1..cmd_n>`, from the
  newest telemetry: the n rpms, then the n commands) and a `C <event_time_s>` line per collision event. Text, LF,
  shortest round-trip numbers; a crash costs at worst the last second or so of audio (the log is buffered), ending in a
  truncated line that is ignored, and the clip ends at the shorter of audio and video. The `mp4` export replays it through `ofs_audio::Audio` (the flight sound of the game, 882 samples
  per frame) into a temporary raw PCM file next to the output, `.<name>.pcm-<pid>.tmp`, which is removed after the export (about
  26 MB for a five-minute flight). A recording without a `.audio` file, or with no frames in it, exports a silent
  MP4; a malformed one fails the export and keeps the AVI and the `.audio` file. The `frames` export is silent.
  The PCM is the same for the same file on one machine, not byte-identical across operating systems, and the AAC
  bytes depend on the ffmpeg build. The sound is the quad's microphone sampled when a frame is written, so it leads the
  picture by the goggles' latency (40 ms by default); it ignores the "Sound" setting and has no VTX buzzer (the sim has no
  buzzer model).

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
   recording running across the switch keeps going, letterboxed to the size it started with.
4. On the Tramp quad, open the OSD stick menu (as in the M3a check) and change the VTX power: the HUD VTX line
   snaps to the built-in table. A band/channel change does nothing — Tramp is frequency-only; the live test
   pins the frequency path.

## The fault catalog (M4b)

The client and server speak **protocol 17**. The fault catalog arrived in protocol 8, and its wire shapes live in
the `Fault` oneof of `proto/ofs/v1/sim.proto`; the injectable Python dataclasses are in `python/ofs/faults.py`
(`sim.inject(...)` / `sim.clear_faults()`), and the models, constants and their estimate status are in
`docs/research/faults.md` —
regenerate the stubs with the table's protoc recipe after changing the proto.

## Recording, scripted inputs and the package (M4c)

The client and server speak **protocol 17**: `StartRecording`/`StopRecording`, `PlayInputs`/`StopInputs`,
`StartInputsRecording`/`StopInputsRecording` (M5), a fault's optional `at_s`, world names in `LoadRequest.world_path`,
`HandshakeReply.logs_dir`, gates with lap timing (`World.gates`, `State.race`, M8a), the stabilizer flags
`LoadRequest.open_loop_fc`/`LoadRequest.stabilize` (M8b), the `DownlinkLoss` fault (`Fault.downlink_loss = 10`,
protocol 13), the loaded parts' provenance (`LoadReply.parts`, protocol 14; Python `sim.parts`), gate tilt and clipped
passes (`Gate.pitch_deg`/`roll_deg`, `EVENT_KIND_GATE_HIT`, protocol 14), and the lap limit and race reset
(`RaceState.finished`/`lap_limit`, `EVENT_KIND_RACE_FINISHED`/`RACE_RESET`, `ResetRace`, protocol 14), and named
courses with a race mode (`World.courses`/`default_course`, `RaceState.course`/`course_index`/`mode`/`time_limit_s`/
`time_left_s`, `ResetRace` taking a `ResetRaceRequest{course}`, protocol 15; Python `sim.reset_race(course)`), and the
baked terrain grid (`World.terrain`: origin, cell size, node counts, heights and the digest, protocol 16). With a grid a
reply can pass 4 MiB, so the Rust and Python clients accept messages of up to 16 MiB (`ofs_proto::MAX_RESPONSE_BYTES`), and the
world's triangle meshes (`World.meshes`: name, absolute path, placement and the file's digest, never the bytes; the Godot
client loads each `.glb` itself and `get_world()` reports whether it matches, protocol 17).

- **Recordings** land in `<data_dir>/logs/<quad stem>-<n>.mcap`; the server names them and never writes a
  client-supplied path. The recorder (`crates/ofs-log`) is the last model of each tick and only reads the bus. A
  record carries the state `GetState` reports at its time (`{"v": x}`, `{"x", "y", "z"}` or `{"w", "x", "y",
  "z"}` JSON with a JSON schema); the default rates are `ofs_log::DEFAULT_RATES`. The files are deterministic:
  the `mcap` crate is pinned to an upstream revision that writes its summary in id order
  (`docs/research/mcap-determinism.md`), and the writer pins its library string and single-threaded zstd. It keeps
  the per-message index records: Foxglove refuses a file without them. The summary section and footer are
  written only when the recording stops; `ofs.reader` refuses an in-progress file (Foxglove untested), and a
  streaming reader sees only its flushed chunks (`docs/research/mcap-determinism.md`).
- **Python:** a lockstep `load` starts a recording (`record=False` does not), `sim.log` stops and returns it,
  `ofs.reader` reads any recording (`mcap` + `pandas`; Parquet also `pyarrow`), `ofs.rerun.view(sim)` is the live
  view (`rerun-sdk` 0.39.x, pinned by the extra).
- **Scripted inputs** (`sim.play_inputs(path)`): the client sends the CSV's text (the server accepts requests up
  to 64 MiB, `MAX_REQUEST_BYTES`, so a long flight's file fits); the server validates it once
  (`crates/ofs-sim/src/inputs.rs`) and applies each row on the first tick at or after its time. The columns are
  `t_s`, any of the eight sticks, and the optional `tx` and `link_loss` (0/1); at least one input column is required,
  a column a file leaves out keeps its current value, and the last row holds after playback. Faults with `at=`
  use the same tick rule. Both apply right after the step that reaches their time, so a `run` that ends there
  already shows them.
- **Worlds by name:** `load(world="flat")` reads `worlds/flat.toml` beside the quad's directory, as parts resolve
  against `library/` (or every root in `OFS_LIBRARY`, first match per part id).
- **Blackbox:** the shipped diffs set `blackbox_device = VIRTUAL`; SITL writes `LOGnnnnn.BFL` into the quad's
  firmware directory, and the session copies the logs it wrote into the logs directory when it ends.
- **Packaging:** the PyPI distribution is `open-fpv-sim` (import `ofs`), pure Python; `launch()` still needs an
  `ofs-sim` binary. The `publish` CI job runs on `v*` tags only, checks the tag equals `python/pyproject.toml`'s
  version and uploads through PyPI trusted publishing (the `pypi` environment); no token is stored.

## Swappable radio parts and recording a live flight (M5)

The radio is three parts (a handset, a link, a receiver) behind `RadioChain`; the quad file's `[radio]` section picks
them (`kind = "elrs"` or `"fixed"`, `handset`, `receiver`, and `band = "2g4"` or `"900"` for ELRS). The shipped
examples are `quads/opendrone-5f-900mhz.toml`, `quads/opendrone-5f-fixed-link.toml` and `quads/opendrone-5f-ibus.toml` (a FlySky iBUS receiver, `receiver = "ibus"`, which needs a SITL built from the current patch); `docs/writing-a-radio-model.md`
shows how to write a part, and `docs/research/serial-radio.md` records why M5 has no real serial radio. Quad files
written before M5 load unchanged and fly byte-identically: the goldens (`crates/ofs-radio/tests/golden.rs`,
`crates/ofs-sim/tests/radio_golden.rs`) pin the ELRS link's bytes, one digest file per OS
(`tests/golden/*.windows.digests`, `*.linux.digests`, `*.macos-<arch>.digests`: `libm` differs between them, so run the
goldens on each when you touch the radio — the Linux ones in WSL; the macOS files come from the survey's
`write_goldens` input).

Both link kinds share `[radio] telemetry_slot_divisor` (default 4): one downlink packet in that many carries a
telemetry frame when heard. It accepts 1, 2, 4, 8, 16, 32, 64 or 128; there is no 0 (off), and one value applies
to every band and packet rate. It changes only the telemetry slot cadence — packet loss, uplink delivery and downlink
LQ stay the same.
The default 4 keeps 0.2.0 recordings and the radio goldens byte-identical.

**Recording a live flight:** `StartInputsRecording` writes the session's sticks and link state to
`<data_dir>/logs/<quad stem>-inputs-<n>.csv` (a row whenever a stick, `tx` or `link_loss` changes, in `play_inputs`'
format) and `StopInputsRecording` returns `{path, rows, duration_s}`. From Python: `path = sim.record_inputs()` and
`sim.stop_inputs_recording()`. In the game: the **I** key toggles it (a toast names the file, and `REC STICKS` shows
in the status line while it runs). A lockstep session replays the file with `sim.play_inputs(path)`; two lockstep
replays of one file write byte-identical MCAP. Files written before 0.3.0 have no `tx` or `link_loss` column and still
play. A replay is not the real-time flight (timing, the SITL schedule and the link's seeded draws differ). Load and
Unload stop the recording and publish `inputs_recording_stopped`; a session that vanishes (its last watcher left, the
server shut down) finishes the file without an event.

M5 manual check (needs a controller; the maintainer's TX15 in USB joystick mode is the reference):
1. `godot --path godot`, bind the sticks in F2, fly with Betaflight, press **I**, fly 20 s, press **I**.
2. In Python, in a lockstep session: `sim.load(quad)`, `sim.play_inputs(<the toast's file, under the logs dir>)`,
   `sim.run(20)`, and watch the replay in Rerun (`ofs.rerun.view(sim)`) or the MCAP in Foxglove.
3. Press **I**, then **R** (reload): `REC STICKS` clears and the next **I** starts a new file.

Checked by hand, 2026-10-16 (joystick, real Betaflight in WSL): steps 1 and 3 behave as written. A 20.6 s flight recorded
3,580 rows. Step 2 in a lockstep session with real Betaflight needs the boot first: `sim.run(6)`, then `play_inputs`,
then `run(<the file's last t_s>)`; the flight replays (a climb to about 47 m, motors idling at the end, link up). Replays
of one file are byte-identical in MCAP when they start from a saved EEPROM; the first replay in a fresh data directory
differs, because that first Betaflight boot flashes the EEPROM and applies the diff. Close the game before replaying
with Betaflight: the lockstep session's SITL cleanup kills the game's Betaflight.

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

## Native Windows SITL (experimental, no WSL)
The pinned Betaflight SITL also builds and runs as a native Windows program, through the Cygwin-based `msys`
environment of MSYS2 (issue #67; the findings are in `docs/research/sitl-native-windows.md`). It measured about
2.3 times faster than SITL in WSL 2 in one benchmark run. WSL stays the supported path. CI builds it and runs the live tests against it (the `windows-sitl` job, in `publish.needs` since 2026-10-31).
Windows may ask whether to allow `betaflight_SITL.exe` on the network the first time it starts (it listens on its
Betaflight ports on every interface): answer Cancel, the simulator talks to it over loopback, which is not filtered.

1. Install MSYS2: `winget install --id MSYS2.MSYS2 -e`, then in an "MSYS2 MSYS" shell
   `pacman -Syu` (twice, the first run restarts the shell) and
   `pacman -S --needed base-devel git patch diffutils python gcc make`.
2. From that shell, in the repository: `bash scripts/build-sitl-windows.sh` (3 to 4 minutes; stop any running native
   SITL first; the directory must not contain spaces, so a Windows user name with a space needs `BF_DIR=/c/ofs/betaflight`;
   it clones the pinned tag to `~/ofs/betaflight`, applies `ofs-sitl.patch` and `ofs-sitl-windows.patch`, builds without link-time
   optimisation, and copies the two Cygwin runtime DLLs beside the executable). It prints the line to use:
3. In PowerShell: `$env:OFS_SITL_LAUNCH = "C:\msys64\home\<you>\ofs\betaflight\obj\main\betaflight_SITL.exe"`, then run
   the live tests or the simulator as usual (the `ofs-fc` and `ofs-video` live tests split the variable at spaces, so
   keep the path free of them). Keep the DLLs beside the `.exe` (a start-up error naming `msys-2.0.dll` means they are
   missing).

The build script runs make in the C locale and, when the link fails on undefined symbols, lists them and points at
`src/platform/SIMULATOR/win_stubs.c` (in `ofs-sitl-windows.patch`): a new pinned Betaflight tag that adds code the PE link
keeps needs its hardware-only symbols stubbed there. In CI the same list is an annotation on the run, and the job uploads
the `.exe` and its two DLLs as the `betaflight-sitl-windows` artifact (for a manual run, not a release).

The default cleanup on a plain Windows host is a PowerShell command scoped to the session's slot (about 0.2 s before
a launch and after a stop when no SITL is left over, and bounded to 10 s); `OFS_SITL_CLEANUP` still overrides it. The
live Rust tests that kill SITL with `pkill` are Linux-only and do not run natively, and the Windows decoy tests
(`crates/ofs-fc/tests/sitl_windows.rs`) take down any real native SITL running in slot 0 or 1, so do not run them
beside a live session.

## macOS
The Rust core, the Python package and Betaflight SITL build and pass their tests on macOS (issue #68; the survey is
`docs/research/macos.md`). CI covers Apple silicon (`macos-latest`, not yet a required check); the Intel survey now runs
all four Godot suites, and the latest run passed them. Nothing was tried on a physical Mac: no window, rendering or game
controller.

1. Install the Xcode command line tools (`xcode-select --install`: clang, make, git), Rust (`rustup`) and Python 3.10+.
2. Rust and Python as everywhere else: `cargo test --workspace`, `python3 -m pip install -e "python[dev]"`,
   `cargo build -p ofs-sim && python3 -m pytest python/tests`. (On an Intel Mac the `[dev]` extra leaves out `rerun-sdk`, which has
   no wheel for it, and the Rerun test skips; the `[rerun]` extra itself cannot be installed there.)
3. Betaflight SITL: `bash scripts/build-sitl.sh` (about a minute; it applies `ofs-sitl.patch` and, on Darwin,
   `ofs-sitl-macos.patch`), then `export OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf` for the live tests and
   the Python SITL tests.
4. Godot: `bash scripts/run-godot-tests.sh all` downloads the macOS editor (checked against a pinned SHA-512) and runs the
   unit and end-to-end suites headless; with `OFS_SITL_LAUNCH` set the game flies Betaflight.

For a physical-Mac check (window, Metal, gamepad, Gatekeeper), follow `docs/research/macos-manual.md`.

Known differences: the radio goldens are compared per architecture on macOS (`*.macos-<arch>.digests`, recorded by the
survey's `write_goldens` input); `replies_from_another_address_are_ignored`
needs `sudo ifconfig lo0 alias 127.0.0.2` and is ignored; a held TCP port does not stop SITL's own bind (the simulator's
UDP 9003 probe catches a stale SITL instead); a window, rendering and a game controller on a real Mac are untested.

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
- **Directory:** each quad file gets `<data_dir>/<quad file stem>-<8 hex digits>/` (the digits hash the quad file's path), holding `eeprom.bin`, `betaflight.diff` (the copy applied on first boot), `sitl.log`, a `config.lock` held while a first boot applies the diff (removed when it ends; deleting a leftover is safe when no first boot is running) and any Blackbox logs.
- **Diff changes:** the quad's diff is applied only on first boot, so Configurator changes persist. If the quad's diff changes later, loading fails until you delete `eeprom.bin` (which also discards Configurator changes). M4c changed both shipped diffs (`set blackbox_device = VIRTUAL`): delete the `eeprom.bin` of each quad you flew before it, once.
- **Reboots:** a Betaflight reboot (`MSP_REBOOT`, the Configurator's Reboot, flashing) relaunches SITL from its EEPROM; `State.fc_restarts` counts them. The PID tab's Save is not a reboot (see "Betaflight Configurator" below).

## Betaflight Configurator
SITL serves MSP on its slot's TCP port (`tcp://127.0.0.1:5761` for slot 0, +10 per slot; also from Windows, through WSL's localhost forwarding), but only while simulated time advances. So connect while a real-time session runs:
1. Start `python python/examples/serve_realtime.py` (with `OFS_SITL_LAUNCH` set). It prints the Configurator address.
2. In the Betaflight Configurator desktop app, enable manual connection in the options, enter the address the script printed (`tcp://127.0.0.1:5761` for slot 0), and press Connect.
3. A Save persists the change to EEPROM. The PID tab's Save does **not** reboot Betaflight (checked by hand, 2026-10-14: a change survived a reconnect and the script's `betaflight restarts` stayed 0). A reboot sent to the port does make the simulator relaunch SITL from its EEPROM — verified live: an `MSP_REBOOT` sent to the Configurator port relaunched SITL in about 1.6 s, the radio link came back up, and the Configurator reconnected.

The web Betaflight App cannot open raw TCP connections.

Persistence over MSP is covered live by `test_a_pid_saved_over_msp_persists_through_a_reboot_and_a_new_session`
(`python/tests/test_sitl_video.py`). To check it with the Configurator application itself, connect as above, then:
1. change one PID value and press Save;
2. reconnect and confirm the value persisted;
3. the script's `betaflight restarts` stays 0 (the PID tab's Save writes EEPROM without rebooting).

## Performance
`scripts/benchmark.py` measures the real-time factor (simulated seconds per wall second) of headless lockstep runs
of the shipped quad: open loop and with Betaflight SITL, recording on and off. The parent spec's target (§8.6) is
at least 2× on a mid-range PC. CI's sitl job runs it against a release build, prints the table in the job summary and
checks two things:

- **The floor:** a row below 1× real time always fails (`--floor 1`).
- **The regression gate (#61):** each row is compared with `scripts/benchmark-baseline.json`, the median of five green
  CI runs on GitHub's `ubuntu-24.04` runner (open loop 47.1× with recording and 131.9× without, Betaflight 10.0× and
  11.8×; the runs agreed within about 6 %). A row under 60 % of its baseline fails, so a slowdown from 12× to 7× is caught
  while the ~6 % runner noise is not. Each row is the best of two runs (`--repeat 2`), because noise only ever slows a
  run down. A skipped row, or a row the baseline does not list, is never gated. The summary table shows the baseline
  and the ratio of every row.

To accept a slowdown on purpose, make it visible: either update the baseline in the same PR (run the benchmark in CI,
or `python scripts/benchmark.py --repeat 2 --print-baseline` on the same kind of runner, and copy the numbers into the
file), or set the repository variable `OFS_BENCH_ACCEPT_SLOWER` to the reason; the table then says
`ACCEPTED: <reason>` for the regressed rows and the job passes. Delete the variable once the baseline PR has merged:
while it is set it accepts every future regression, not just the one it named. A developer machine is not a runner class: run the
benchmark there without `--baseline` (the baseline is in numbers that only `ci-linux` can reproduce).

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
On CI's shared Linux runner (native, no WSL) the Betaflight rows sit at 10.0× and 11.8× — the medians the baseline
file records from five green runs of 2026-10-17 (an earlier single-run measurement, run 38453432260, said 12.4× and 13.8×).

### Reading recordings
`scripts/bench_reader.py` records a minute of open-loop flight and times `ofs.reader.read` against the `mcap`-iterator
reader it replaced (issue #45), checking that both return the same thing. On the Windows development machine with a
debug server (27.6 MB, 148 signals): three patterns (six signals) 3.29 s to 0.34 s (9.6×), every signal 9.76 s to 2.24 s (4.3×); under
WSL 2, 2.32 s to 0.29 s (8.0×) and 8.33 s to 2.08 s (4.0×). The
fast path needs message indexes and zstd or uncompressed chunks, which every recording the server writes has; other
files fall back to the iterator.

## Ports and parallel sessions
Several simulators can run Betaflight side by side on one machine (each `ofs.launch()`, or each `ofs-sim` process, is one
session). Every SITL runs on a *slot*, a block of ten ports (SITL's `--port-offset`, which the simulator passes itself):

| Slot | UDP (raw PWM, motors, state, RC) | TCP (UART1 = the Configurator's MSP, UART2…) | Configurator address |
|---|---|---|---|
| 0 | 9001–9004 | 5761… | `tcp://127.0.0.1:5761` |
| 1 | 9011–9014 | 5771… | `tcp://127.0.0.1:5771` |
| n (up to 15) | 9001 + 10 n … | 5761 + 10 n … | `tcp://127.0.0.1:` 5761 + 10 n |

The first session on the machine gets slot 0, exactly the ports of earlier releases; the next one gets the first slot
that is free. A simulator takes a slot by holding two sockets for the life of its session, a loopback UDP lock on
`21600 + n` and the motor socket on `9002 + 10 n`, which the OS releases when the process ends however it ends, so a
crashed simulator frees its slot. It also probes the slot's TCP UART block (`5761 + 10 n` and the seven above it,
which Betaflight binds at launch) bind-and-release on the address its own sockets use, so a foreign holder there
skips the slot instead of turning the launch into a bind failure (under WSL the probe watches the Windows side,
where the OS's loopback relay for the VM's SITL must not count as a holder). A bind that fails for another reason
(permissions, say) is named in the error when no slot can be taken; with all 16 taken the load fails with an error
that says so. A session rebuilt over itself (a reload) keeps its slot instead of racing for the first free one, so
its EEPROM directory stays its own. `LoadReply.configurator_address` (Python: `sim.configurator_address`, the
game's toast) always says which address the session got.

- **The cleanup is per slot.** Before a launch and after a stop, the simulator removes stray SITL processes *of its own
  slot* (the processes named `betaflight_SITL` whose command line carries the slot's `--port-offset`, or none at all for
  slot 0, which is how an older simulator started it; a path with spaces is no trouble; under WSL the same, inside the
  VM), so starting or stopping one session never kills another. `OFS_SITL_CLEANUP` still replaces it with whatever you
  give; a command that kills every SITL by name, such as `pkill -x betaflight_SITL`, brings back the one-session-per-machine
  rule. The simulator owns the option: a `--port-offset` written into `OFS_SITL_LAUNCH` or `fc.launch` is replaced by the
  slot's.
- **The firmware directory is per slot.** Slot 0 uses `<data dir>/<quad>-<hash>` as before; slot n > 0 uses
  `<data dir>/<quad>-<hash>-s<n>`, so two live SITLs never share an EEPROM, `sitl.log` or Blackbox log. A new slot's
  directory starts empty: its first boot applies the quad's diff like any new directory, and the first boot of a fresh
  directory differs from later ones (compare runs from a warmed-up directory, as the hover test does). Two simulators
  that use different data directories never collide anyway (`ofs.launch()` defaults to `.ofs-data` in the working
  directory, so sessions started from the same folder share it and use the slot suffix).
- **Rebuild SITL** with `scripts/build-sitl.sh`: the simulator always passes `--port-offset`, and a SITL built before this
  option answers `[SITL] Unknown argument: --port-offset` and exits, which is reported as a startup error.
- **Limits.** Slots are claimed on the simulator's side of the machine: a simulator on Windows and a simulator running
  inside WSL do not see each other's locks, so both can pick slot 0, and the second one's cleanup then removes the
  first one's SITL inside the VM (the same slot there); the first session fails later with "no motor output". Run all
  your simulators from one side (all on Windows with `wsl.exe` launches, or all inside WSL). Each session is still one
  simulator process.
  The Rust live tests keep running one at a time (`--test-threads=1`): several of them kill every SITL by name on
  purpose. `python/tests/test_sitl_parallel.py` and `a_second_session_gets_its_own_slot_and_both_fly` are the tests of
  parallel sessions.
