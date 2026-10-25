# Changelog

All notable changes, by release of the Python package `open-fpv-sim` (the simulator is built from this
repository). The format follows Keep a Changelog.

## Unreleased

### OSD warnings and criticals (part of #32)
#### Changed
- **The Godot OSD draws Betaflight's warning and critical severities.** Cells on font page 2 (WARNING) and page 3
  (CRITICAL) were drawn like ordinary text; they now use the bold glyphs in amber and red. No new glyph art or licence
  change. The HD/digital element set of #32 is not done: it needs an HD SITL build (see `docs/roadmap.md`).

No protocol, server or Python change. **Rust and Godot API:** `Client::dvr_start`, `dvr_write_frame` and `dvr_stop`
now take a `DvrFeed` (`Goggles` or `Clean`), and the Godot `OfsClient.dvr_start`, `dvr_write_frame` and `dvr_stop` take
the feed name (`"goggles"` or `"clean"`); `OfsClient.dvr_export(path, mode)` and `dvr_export_poll()` are new.

### A DVR that survives a resize (#34)
#### Changed
- **A window resize or an aspect switch no longer stops the recording.** Frames are scaled to fit inside the size the
  recording started with and centred on black bars. Entering the chase view, a reload and a lost session still stop it.

### A clean-feed DVR (#35)
#### Added
- **`dvr_feed` (`goggles`, `clean`, `both`)**, a button on the F2 screen, `OFS_DVR_FEED` and `--dvr-feed=`. `clean`
  records the camera image without the OSD and the video-link effects into `REC_<ts>_clean.avi`; `both` writes it
  beside the goggles file `REC_<ts>.avi`. Both files write on every field (a black clean frame stands in where the
  clean picture does not exist yet), so their frame counts are equal unless the stop toast reports dropped frames. An
  invalid value means `goggles`.

### Image-sequence and MP4 export (#37)
#### Added
- **`dvr_export` (`none`, `frames`, `mp4`, `both`)**, a button on the F2 screen, `OFS_DVR_EXPORT` and `--dvr-export=`.
  After a recording with frames, `frames` copies the AVI's JPEGs unchanged to `<name>_frames/frame_000001.jpg...` and
  `mp4` runs the `ffmpeg` on `PATH` (it needs libx264; without ffmpeg a toast says so and the AVI is kept). Nothing is
  overwritten, and the work runs on a worker thread. An invalid value means `none`.

## 0.4.0 — 2026-10-24

**Upgrade:** protocol 14 (rebuild/reinstall the client and server together); a protocol-13 Python client or server
is refused at the handshake. In a world with gates, a pass now needs room for the quad (a clipped pass raises
`gate_hit` and no longer counts), and the bus digest covers the new `race.finished` signal, so a repro bundle of a
gate world recorded by 0.3.0 no longer reports `reproduced=true` — re-record it on 0.4.0. A bundle's journal can now
hold a `race_reset` line, which a 0.3.0 build refuses to read. Worlds without gates, and every golden, are unchanged.

### A repro bundle keeps the EEPROM as the session failed (#106)
#### Changed
- **`firmware/eeprom.bin` (and `betaflight.diff`) are read when the bundle is written, not at `Load`**, so a setting the
  Configurator saved after `Load` is in the bundle and a replay's Betaflight boots with it. The file is read twice and
  kept when both reads match; a missing or unsettled file falls back to the `Load`-time copy. `bundle.toml` gains
  `firmware_captured` (`"failure"` or `"load"`) for Betaflight sessions. A replay flies the whole journal with the
  failure's settings (`docs/repro-bundle.md`, "The firmware's settings"). Settings changed but not saved never reach the
  EEPROM and are not captured.

### Race gates: the quad's size and tilted openings (#103)
#### Changed
- **A pass has to fit the quad.** The crossing point must be at least the quad's bounding radius (its collision
  spheres' farthest edge, 0.2 m for the shipped quad) inside the opening's edges. A pass whose centre is inside the
  opening but closer to an edge raises the new `gate_hit` event (protocol 14, `EVENT_KIND_GATE_HIT`; Godot toasts it),
  gives no credit and leaves the gate next. A gate counts again, as after any pass, only once the quad has been 3 m
  from its plane. A course whose openings are wider than the quad by more than its radius, like `worlds/circuit.toml`,
  is unaffected.
#### Added
- **`pitch_deg` and `roll_deg` on a `[[gates]]` entry** (default 0, an upright gate; aerospace Z-Y-X after the
  heading). The crossing test, the ground check and the Godot frame use the tilted plane; `Gate.pitch_deg` and
  `roll_deg` on `World.gates` (protocol 14, Python `ofs.Gate`). Worlds without them behave exactly as before.

### Race: lap limit, reset and next-gate highlight (#104)
#### Added
- **`[race] laps = N` in a world file** ends the race after N laps: `lap_completed`, then the new `race_finished`
  event, the clock stops and nothing counts until a reset. `State.race` gains `finished` and `lap_limit`, and the
  `race.finished` signal joins the other `race.*` ones (so a recording of a world with gates has one more channel).
  Worlds without `[race]` race forever, as before.
- **`ResetRace`** (protocol 14; Python `sim.reset_race()`, the **N** key in the game) starts a fresh race at the next
  tick without a `Load`: no clock, laps or times, gate 0 next, a `race_reset` event, the `race.*` signals back to zero.
  A world without gates refuses it. A repro bundle's journal records it (`race_reset`).
- **The game highlights the next gate** (green and stronger; the start/finish gate stays yellow until it is next), shows
  `LAP 2/3` with a limit and `RACE FINISHED` at the end, and toasts both events.

### Part provenance over the API (#43)
#### Added
- **`LoadReply.parts`** (protocol 14) lists the library parts the quad loaded — frame, motors, props, escs, battery
  and vtx, a single motor/prop/esc id once — with the provenance of every parameter (`datasheet`, `measured` or
  `estimated`, the link and the note). Python: `sim.parts` after `load()` (`ofs.Part`, `ofs.Provenance`);
  `part.estimated` names the parameters that are only guesses. Rust: `QuadConfig::part_provenance`.

### Reader error for a still-running recording (#117)
#### Changed
- **`ofs.reader.read` on a file that is still being recorded** says so: the "unfinished recording" error now names the
  running recording as the likely cause and the way out (`sim.recording.stop()`, or `sim.log`, which stops it).

### Tests
- The open-loop Godot end-to-end test waits for a current field instead of judging one frame, and the DVR check owes no
  frames for boundaries held before the first fresh one (#118: the flake was seen on the macOS job only; the issue stays
  open until that job has a clean record).

## 0.3.0 — 2026-10-22

**Upgrade:** protocol 13 (rebuild/reinstall the client and server together); a protocol-12 Python client or server
is refused at the handshake; `OFS_LIBRARY` may now be a list; the inputs CSV gains optional `tx`/`link_loss`
columns; `radio.telemetry_slot_divisor` defaults to 4; a new `DownlinkLoss` fault (the bus digest covers the new
`fault.radio.downlink_loss_fraction` signal, so a repro bundle recorded by 0.2.0 no longer reports
`reproduced=true` — re-record bundles on 0.3.0); the F2 DVR toggle.

The release also carries the work that landed while it was open — parallel SITL sessions (M9a), the experimental
native Windows SITL (M9b), macOS support (M9c), the stabilized-session and hardening sweeps (B2 to B5), and the
audit follow-ups — the detail by item follows.

### The component library is a search path (#44)
#### Changed
- **`OFS_LIBRARY` may list several roots**, `;`-separated on Windows and `:`-separated elsewhere (empty entries are
  dropped; a blank value means the default, the library beside the quads). Each part id resolves against the first
  root that has it, and a part no root has is refused naming every root searched. A repro bundle merges the roots
  the session read at `Load` into its `library/` first-match-wins (a root that has gone missing is skipped), and a
  replay builds the bundle's own library — the replayer's `OFS_LIBRARY` never reaches it.

### A configurable telemetry slot ratio (#22)
#### Added
- **`[radio] telemetry_slot_divisor`** (both link kinds, default 4): one downlink packet in that many carries a
  telemetry frame when heard. It accepts 1, 2, 4, 8, 16, 32, 64 or 128 — there is no off value — and one value
  covers every band and packet rate. It changes the telemetry cadence only: packet loss, uplink delivery and
  downlink LQ are the link model's own. The default 4 keeps 0.2.0 recordings and the radio goldens byte-identical.

### Downlink telemetry loss
#### Added
- **`DownlinkLoss(fraction)`** drops `fraction` of the handset's telemetry packets while leaving the uplink, sticks
  and failsafe untouched. Protocol 13 adds `Fault.downlink_loss` (oneof field 10, `at_s` stays 9) and
  `ofs.faults.DownlinkLoss`; default MCAP recordings leave the new control signal out, so a 0.2.0 no-fault
  recording stays byte-identical.

### Link state in the recorded sticks (#56)
#### Changed
- **The inputs CSV gains optional `tx` and `link_loss` columns** (0/1; with `t_s` and the eight sticks, eleven
  columns in all). A recording writes a row whenever a stick, the transmitter or the link changes — a **K** cut with
  the sticks still is recorded — and `play_inputs` applies both through the same setters a live session uses, so the
  recorded flight round-trips its bus at each tick. A 0.2.0 stick-only file still plays unchanged; a file with the
  new columns is refused by a 0.2.0 server (an unknown column is a header error).

### A DVR toggle in the game (#38)
#### Added
- **The F2 controls screen has a DVR checkbox**, one start/stop path with the **D** key: either one moves the other,
  an automatic stop (a picture-size change, such as a resize) unchecks the box, and the HUD shows `REC` while a
  recording runs.

### Reading a recording while it is written (#48)
#### Added
- The answer, probed and written down (`docs/research/mcap-determinism.md`): a recording's summary section, footer
  and tail magic are written only by `stop()`, so `ofs.reader` refuses the live file ("unfinished recording") and no
  indexed reader can build its view until then — a streaming reader sees only the flushed chunks. Call
  `recording.stop()` (or `sim.log`, which stops and returns it) first. Foxglove itself was not installed on the
  evidence machine, so its behaviour on a live file is unchecked — #117.

### Replaying a bundle no longer runs its programs (#108)
#### Security
- **`ofs-sim --replay` and `ofs.repro.replay_bundle` do not run the programs a bundle's quad file names** (`fc.launch`,
  `fc.cleanup`): a repro bundle is whatever someone attached to an issue. A Betaflight bundle replays with
  `OFS_SITL_LAUNCH` set to your own SITL, and is refused without it; `--trust-bundle` (Python `trust_bundle=True`) runs
  the bundle's own programs. A bundle that asks for more than a simulated day of ticks is refused instead of spinning.

### M9c: macOS (#68)
#### Added
- **macOS survey and support (best effort).** Betaflight SITL builds and runs on macOS with `third_party/betaflight/ofs-sitl-macos.patch`
  (applied by `scripts/build-sitl.sh` on Darwin; the realtime clock replaces `pthread_condattr_setclock`), the slot cleanup
  finds stray SITLs with `ps` where there is no `/proc`, `scripts/run-godot-tests.sh` runs the macOS editor, and CI has a
  `macos` job (Apple silicon: Rust tests, live Betaflight tests, Python, Godot with the game flying Betaflight). The full list of what
  builds and what does not, with run ids: `docs/research/macos.md`. A manual `macos-survey` workflow reruns the survey on
  Apple silicon and Intel.
#### Changed
- Tests that assumed GNU/Linux no longer do (`touch -d`, a one-shot read of the event stream, a tight timing window); one
  test that needs a second loopback address and one that needs a held port to stop SITL are skipped on macOS (the second
  returns early, so it shows as passed); `[dev]` leaves out `rerun-sdk` on Intel Macs, which it has no wheel for.
- The cleanup of a SITL launched without a slot is a `ps`-based script on macOS, where `pkill -x betaflight_SITL` finds nothing.

### Audit follow-ups (2026-10-22, `docs/superpowers/plans/2026-10-22-audit-smaller-fixes.md`)
#### Fixed
- **Two simulators sharing a data folder can no longer collide on a log name**: an MCAP recording, an inputs CSV or a
  repro folder whose number another process took first is created under the next number.
- **`ofs.launch()` no longer picks a port and hopes**: the server binds first (`--listen 127.0.0.1:0`) and reports its
  port through the new `--port-file FILE` option, so a launch cannot end up on another process's server.
- SITL output is read in lines of at most 64 KiB, so a child that prints without newlines cannot grow memory.
#### Added
- A test that the committed Python stubs match `proto/ofs/v1/sim.proto`; Dependabot for the workflows' actions, and the
  publish job's actions pinned to commit SHAs.

### B5: the small-tails sweep
#### Fixed
- **`ofs-validate` reports every `submission.toml` schema problem in one pass** (unknown keys, wrong types, per key),
  instead of stopping at the first; a file that does not parse still says so with its line.
- **Ending a session stops a running MCAP recording with a named `recording_stopped` event**, exactly as a running
  inputs recording does; the file is finished, not left half-written.
- The live SITL tests kill their own session's SITL by slot (the same selection the product's cleanup makes) instead
  of `pkill -x betaflight_SITL`, which took down every SITL on a developer machine; the crash test holds a decoy SITL
  in another slot across the kill to prove the scoping.
- The victim tests' fake SITL processes are killed when a test's assertion fires (a drop guard), so a failing test
  no longer leaks a process holding the ports.
- The release checker's changelog walk catches an empty `## Unreleased` section and a `###` heading outside any
  release; `docs/dev-setup.md`'s protocol-12 line names the stabilizer flags.
#### Removed
- A redundant journal-replay stick-clamp test that passed with the clamp removed (the clamp is pinned by
  `set_sticks_clamps_what_it_accepts`).

### B4: race polish
#### Fixed
- **The HUD's lap clock rounds before it splits minutes**, so a lap of 59.96 s shows `1:00.0`, never `0:60.0`.
- **Race events keep their crossing time inside a `changes()` batch**: the batch is time-ordered (a stable sort, so
  events at the same time keep the order they were raised). A gate event now also has its end-to-end test through
  `Session::changes()`, flying the circuit world on the stabilizer.
- The Godot world node's header, `python/README.md`'s race fields (missing `gate_count`) and the race-timing Python
  snippet's note (an unsteered flight prints no events) say what is true; the M8a plan's ledger rules that the exact
  opening edge of a gate is counted.

### B3: benchmark-gate truth
#### Fixed
- **The ci-linux baseline's "open loop, recording" row is 47.1, not 47.9**, and the five runs agreed within about 6 %,
  not 4 % (the M8c review recomputed the medians; the gate moves 28.7 to 28.3, negligible). The docs now name the
  baseline file instead of a superseded run, and the reader benchmark says "three patterns".
- `scripts/benchmark.py` refuses `--runner`/`--min-ratio` without `--baseline` (they were silently ignored), reports
  an unknown runner as a clean error instead of a traceback, and `--print-baseline` prints the rows it could not
  measure as `null` with a warning, so a paste-over cannot silently un-gate the Betaflight rows (a `null` baseline
  row is no baseline).
- **The indexed MCAP reader cross-checks its message indexes** (record opcode and channel, each entry's time against
  its message header, truncation) and falls back to the mcap iterator on any disagreement: a corrupted index can no
  longer return wrong data silently, and index-only damage no longer fails a read the iterator serves.
- `OFS_BENCH_ACCEPT_SLOWER`'s docs say to delete the repository variable once the baseline PR has merged.

### B2: stabilized-session semantics
#### Changed
- **Repro bundles say format 2.** The layout is unchanged, but a 0.2.0-or-older build now refuses the bundles this
  build writes ("this build reads 1") instead of misreading them — for a stabilized session it would have replayed
  the bundle throttle-only and reported "not reproduced"; this build still reads the format-1 bundles 0.2.0 wrote. The bundle also records what the session actually flew
  (a quad file that is `open_loop_stabilized` records `stabilize = true` even when the request sent no flags).
- Loading a quad file whose kind is `open_loop_stabilized` with `open_loop_fc = true, stabilize = false` is refused
  (invalid argument) instead of silently flying it throttle-only.

### M9b: native Windows SITL (experimental)
#### Added
- **Betaflight SITL without WSL (#67).** `scripts/build-sitl-windows.sh`, run from an MSYS2 shell, builds the pinned
  SITL as a Windows program (`third_party/betaflight/ofs-sitl-windows.patch` on top of `ofs-sitl.patch`) and copies
  its two Cygwin runtime DLLs beside it; set `OFS_SITL_LAUNCH` to the `.exe`. Run from Windows it passed the `ofs-fc`
  and `ofs-video` live tests, the `ofs-sim` live tests that run on Windows (9 of 9; 5 more are Linux-only) and the 26
  Python SITL tests, and measured 2.1 to 2.5 times faster than SITL in WSL 2 in one benchmark run. WSL stays the
  supported path: no CI job builds the native one yet. `docs/research/sitl-native-windows.md` has the findings, what is left, and the sizing of a
  DLL-free port.
- The default stray-SITL cleanup on a plain Windows host is scoped to the session's slot (a PowerShell command; it
  returned nothing before), and a SITL that cannot load its runtime DLLs now says so.
#### Changed
- Two Python test helpers (`msp_status.py`, `test_sitl_telemetry.py`) keep pumping simulated time while they wait for an
  MSP reply instead of blocking on the socket, and `test_per_motor_erpm_reaches_betaflight` samples again until the
  four rpm values differ (a single sample failed 5 runs in 8 against the native SITL).
- The cleanup commands of a SITL launch are given up on after 10 s (`CLEANUP_TIMEOUT`) instead of waited for without a
  bound.

### M9a: parallel SITL sessions
#### Added
- **Several Betaflight sessions on one machine (#15).** Each simulator process takes the first free *slot*, a block of
  ten ports: the first session has the stock ports and the Configurator address `tcp://127.0.0.1:5761`, the next
  `5771` (UDP 9011 to 9014), and so on up to 16. The patched SITL takes `--port-offset N` (0 to 1000), the simulator
  claims a slot by holding sockets the OS releases on any exit, and a session in slot n > 0 uses its own firmware
  directory (`<quad>-<hash>-s<n>`). `LoadReply.configurator_address` says which address a session got.
- `PortMap` and `SitlSlot` in `ofs-fc`, `Vehicle::firmware_dir()` and `slot_firmware_dir` in `ofs-sim`, and the tests
  `python/tests/test_sitl_parallel.py` (two `ofs.launch()` sessions flying Betaflight at once, one closing while the
  other flies) and, in `ofs-sim`, `a_second_session_gets_its_own_slot_and_both_fly`.
#### Changed
- **The stray-SITL cleanup is scoped to the session's slot.** The default cleanup, the wait for its victims and the
  SIGKILL escalation used to match every `betaflight_SITL` on the machine by name, so a second session would have killed
  the first; they now match only processes named `betaflight_SITL` launched with the slot's `--port-offset` (slot 0
  also takes one launched without the option, as 0.2.0 did). A `--port-offset` in `OFS_SITL_LAUNCH` or `fc.launch` is
  replaced by the slot's. A launch argv without the option (a hand-made `OFS_SITL_LAUNCH` in a test) and an explicit
  `OFS_SITL_CLEANUP` keep the by-name behaviour.
- A replay (`ofs-sim --replay`) seeds the bundle's firmware into the slot it takes, so it replays exactly while another
  session is running.
- **Rebuild SITL** with `scripts/build-sitl.sh`: the simulator always passes `--port-offset`, so an older SITL binary
  stops at startup with `[SITL] Unknown argument: --port-offset`.

### A dropped Python `Sim` stops its server (#109)
#### Fixed
- **A `Sim` from `ofs.launch()` that was loaded and then dropped without `close()` is collected, and its server and
  Betaflight stop with it.** The thread reading the event stream held the `Sim`, so it lived (and its server ran, and held
  a SITL slot) until the script exited; a loop that launched a server per round ran out of slots after 16.

## 0.2.0 — 2026-10-17 (phases 2 and 3)
Milestones M5 to M8c. Phase 2 made the models more accurate and the simulator verifiable; phase 3 hardened it, added
race timing and made flights scriptable.
- **A swappable radio (M5):** the radio is three parts (`Handset`, `Link`, `Receiver`) with an ExpressLRS link on 2.4
  GHz or 900 MHz and a geometry-free `fixed` link; a live flight's sticks can be recorded to a `play_inputs` CSV (the
  game's **I** key, `sim.record_inputs()`).
- **More accurate physics and electrics (M6a, M6b):** rotor gyroscopic coupling, windmilling, a soft FC mount, a
  per-cell battery with ageing, a thermal model for the ESCs, motors and VTX, and the video receiver's thresholds as
  world data.
- **Verifiable (M6c):** a failed session writes a repro bundle that `ofs-sim --replay` and `ofs.repro.replay_bundle`
  run again; `ofs-validate` is a harness for community measurements (`docs/validation.md`).
- **Hardened (M6d, M6e, M7a):** stricter config checks, validated and clamped sticks, an inputs recording that always
  finishes, a Blackbox log that is finished even when a session ends armed, a Betaflight reboot that no longer wedges
  when the client hangs up, clearer errors from the recording reader, and two radio fixes (a burst-loss window edge,
  the 2.4 GHz 50 Hz sensitivity).
- **Race timing (M8a):** `[[gates]]` in the world file, split and lap events, a lap clock in the state and on the
  game's HUD (`docs/race-timing.md`).
- **Scripted flight (M8b):** an opt-in angle-mode stabilizer for open-loop sessions, a four-gate circuit
  (`worlds/circuit.toml`) and a pilot script that flies a lap on it and on real Betaflight
  (`docs/scripted-flight.md`).
- **Tooling (M8c):** a CI gate against a recorded benchmark baseline, and a recording reader 4 to 10 times faster.

**Upgrading from 0.1.0**
- The gRPC protocol is version 12 (0.1.0 spoke 9): a 0.1.0 client or server is refused at the handshake, so install the
  Python package and build `ofs-sim` from the same release (the tag `v0.2.0`).
- Rebuild Betaflight SITL with `scripts/build-sitl.sh`: the patch changed (a client that hangs up after `MSP_REBOOT` no
  longer wedges the reboot, and the SITL's ports bind reliably). A custom quad whose Betaflight diff failed silently
  on first boot in 0.1.0 now fails loudly: the diff apply is judged on the config run's exit and its whole output.
- Sticks are validated and clamped for everyone: a non-finite stick is refused, an out-of-range one is clamped to the
  transmitter's range (sticks to [-1, 1], throttle to [0, 1]) before the bus sees it, so the `rc.*` channels of a
  recording show the clamped values. In the Rust API `Vehicle::set_sticks` returns a `Result`, and `ofs-radio` was
  restructured into handset, link and receiver parts (M5): code that builds a link by hand needs porting.
- Flights differ from 0.1.0 wherever the new models act: unbalanced rotors (a yaw input, a failed motor) through
  gyroscopic coupling, any descent through windmilling on the shipped prop, out-of-range sticks (now clamped), and
  seeded radio runs with burst loss or ELRS 2.4 GHz at 50 Hz (a window edge and the sensitivity were fixed).
  Recordings are not byte-identical to 0.1.0's, and the golden digests in the repository were regenerated along the way.
  Do not expect to compare a 0.2.0 run against a stored 0.1.0 one.
- Recordings carry new channels: `prop.{i}.inflow_mps`, `battery.cell.{k}.voltage_v` and `.soc`, the temperatures
  `esc.{i}.temp_c`, `motor.{i}.temp_c` (100 Hz) and `vtx.temp_c`. The pattern `battery.*` now matches 16 signals on the
  6S pack instead of 4, and `motor.*` and `esc.*` now include a 100 Hz temperature next to the 242 Hz channels, so a
  `to_dataframe` selection that mixes them has gaps on the union time grid: `.dropna()` a column before plotting it.
  Scripts that count channels or compare recordings byte for byte need a look.
- The shipped `flat` world now has three gates, so a session in it (the README's `world="flat"`) has a race: `race.*`
  channels, `State.race.present`, and `gate_passed` and `lap_completed` events once the quad flies through them. The
  Godot game draws the gates and shows a lap line.
- New opt-in `stabilize=True` on `load(..., open_loop_fc=True)` (a test stand-in for scripted flight, not Betaflight);
  without it open loop is unchanged.
- A Betaflight session that ends armed steps up to 3 more simulated seconds to disarm, so its Blackbox log is finished;
  a running recording sees those steps.
- The loader refuses a prop part with `windmill_j0` between 0 and 0.2 and a battery whose resistance grows more than
  tenfold.
- Every session now keeps a 10 s tail and a journal in memory for the repro bundle (about 15 MB for the shipped quad,
  about 6 % of the open-loop real-time factor).

The detail by milestone follows.

### M8c: a performance gate and a faster reader
#### Added
- **A benchmark regression gate (#61).** CI's `scripts/benchmark.py` step compares every measured row with a baseline
  recorded for its runner class in `scripts/benchmark-baseline.json` (the median of five green runs on `ubuntu-24.04`:
  open loop 47.9× with recording and 131.9× without, Betaflight 10.0× and 11.8×) and fails a row under 60 % of it, on
  top of the old 1× floor. Each row is the best of two runs (`--repeat 2`) so the 4 % noise between CI runs cannot
  trip it; skipped rows and rows without a baseline are never gated. A deliberate slowdown is accepted by updating the
  baseline in the PR or by setting the repository variable `OFS_BENCH_ACCEPT_SLOWER` to a reason (the table then says
  `ACCEPTED: <reason>`). `--print-baseline` prints the measured rows as baseline JSON.
- `scripts/bench_reader.py`: times the reader against the previous implementation on a fresh recording.
#### Changed
- **`ofs.reader.read` is 4 to 10 times faster (#45)**, with the same signature, result and error messages. On a
  one-minute open-loop recording (27.6 MB, 148 signals) three patterns take 0.34 s instead of 3.3 s and every signal
  2.2 s instead of 9.8 s. It cuts only the selected signals' records out of each chunk using the chunk's message
  indexes. Recordings without message indexes or with lz4 chunks (not what the server writes) are read the old way.

### M8b: scripted flight
#### Added
- **An opt-in stabilizer for open-loop sessions (#66).** `load(..., open_loop_fc=True, stabilize=True)` (Rust:
  `FcKind::OpenLoopStabilized`, or `[fc] kind = "open_loop_stabilized"` in a quad file) adds a minimal angle-mode
  controller, so the roll and pitch sticks set the lean (up to 60 degrees), the yaw stick the turn rate (up to 200 degrees
  per second) and a script can fly a path without Betaflight. It is a test stand-in, not Betaflight: it reads the
  simulator's truth and has no arming, rates, I-term or filters, and real tuning needs SITL. `open_loop_fc` alone is
  unchanged (throttle to every motor). `docs/scripted-flight.md`.
- **A scripted circuit (#53).** `worlds/circuit.toml` (four gates on a 40 m square) and `python/examples/circuit.py`, a
  pilot that steers with the sticks from the quad's state and flies a lap through the gates. `python/tests/test_circuit.py`
  asserts the lap on the stabilizer in every CI run; `python/tests/test_sitl_circuit.py` asserts it on real Betaflight in
  the `sitl` job.
#### Changed
- **Breaking (protocol):** the protocol is version 12 (`LoadRequest.stabilize`), so a client of protocol 11 is refused by
  the handshake. `stabilize` without `open_loop_fc` is refused with `invalid_argument`. A repro bundle records
  `stabilize` (a bundle without the key was never stabilized).

### M8a: gates and lap timing
#### Added
- **Race timing (#71).** A world file declares an ordered course with `[[gates]]` (`name`, `center_ned_m`, `heading_deg`,
  `width_m`, `height_m`; at most 64; gate 0 is the start and finish). The simulator counts the gates the quad flies
  through in order, interpolating each crossing inside the tick, and times the splits and laps (`docs/race-timing.md`).
  `worlds/flat.toml` has three gates.
- Events `gate_passed` and `lap_completed` (messages such as `Gate1 (gate 1) split 8.412 s` and `lap 1: 31.204 s (best)`),
  `State.race` (`present`, `running`, `next_gate`, `gate_count`, `laps`, `current_lap_s`, `last_lap_s`, `best_lap_s`),
  `World.gates`, and `race.*` signals in a recording. A world without gates has none of them.
- The Godot client draws each opening as a translucent rectangle labelled START or with its number, shows a lap line on
  the HUD and toasts the events. Python: `ofs.Gate`, `ofs.Race`, `World.gates`, `State.race`.
#### Changed
- **Breaking (protocol):** the protocol is version 11, so a client of protocol 10 is refused by the handshake. The
  Rust and Python clients in this repository speak it.

### M7a: hardening sweep
#### Changed
- **Breaking (Rust API):** `Vehicle::set_sticks` returns a `Result`: a non-finite stick is refused with an `InvalidArgument` error and
  nothing is applied; what is accepted is clamped to the transmitter's range (sticks to [-1, 1], throttle to [0, 1])
  before the bus, an inputs recording and the journal see it (#54). The gRPC `SetSticks` already refused non-finite
  values; out-of-range ones are now clamped on the bus too (the `rc.*` channels of a recording show the clamped values).
- The inputs recorder writes no row for a change that came back to the written row within the same tick, and always
  writes a first row (#54).
- `Load` and `Unload` stop a running inputs recording, so its file is complete, and publish `inputs_recording_stopped`
  (#55).
- A session on Betaflight that ends while the quad is armed disarms it first (the link and the radio faults are
  restored on the bus so the disarm reaches it, then it steps until the motors read disarmed, 3 simulated seconds at
  most), so its Blackbox log is finished; it used to be cut short, notably when the pilot had left first, which is the
  Godot client's quit order. A running MCAP recording sees those steps; the journal and an inputs recording do not
  (#58). A session that is not armed, or has failed, does not pay for it.
- Reading a recording the Python reader cannot use now says why instead of an mcap record error (#59): `unfinished`
  (no footer: a write error such as a full disk, or a killed server), `damaged` (a half-written last chunk) or
  `not an MCAP file`, each pointing at the `recording_stopped` event where relevant. The writer finishes the file
  best-effort after a write error (a test holds that), but the record being written can be left half written.
- Radio: a packet exactly on a burst-loss window edge belongs to the window that starts there, for the ELRS and the
  fixed link alike (#13), and the 2.4 GHz
  50 Hz sensitivity is -115 dBm, ExpressLRS's published figure, instead of -117 (#12). Both change seeded runs and the
  ELRS golden digests (regenerated on both OSes, in their own commits).
#### Fixed
- The patched Betaflight SITL no longer wedges a reboot when the MSP client hangs up right after `MSP_REBOOT` (#57):
  rebuild SITL with `scripts/build-sitl.sh`.
- The SITL port-bind flake (#14) was fixed in the patch on 2026-10-15 (`6ccbaef`, `ac97cad`); no recurrence since.

### M6e: the rest of the M6 minors
#### Changed
- The default recording table records the ESC and motor temperatures (`esc.{i}.temp_c`, `motor.{i}.temp_c`) at 100 Hz,
  the thermal model's own rate, instead of 240 Hz (they only change at 100 Hz). Recordings and the radio-flight
  golden digests change accordingly (both OS files regenerated); the simulation itself does not. A Python selection
  such as `to_dataframe(["esc.*"])` now mixes two rates (242 Hz and 100 Hz), so its columns have gaps on the union time
  grid: `.dropna()` a column before plotting it, as for any mixed-rate selection.
- ESC telemetry and the VTX built without a thermal model (test rigs) read the default 25 °C ambient, not 0 °C.
  `ofs_core::Bus::signal_or` and `ofs_core::consts::DEFAULT_AMBIENT_C` are new.

### M6d: deferred minors
#### Changed
- The loader refuses what it used to accept: a prop part with `windmill_j0` between 0 and 0.2 (0 still turns windmilling
  off; a small value is a runaway rotor), and a battery whose `resistance_growth_per_cycle × cycles` makes the pack more
  than ten times as resistive. A non-finite `[video]` threshold is reported on its own field.
- `ofs-validate`: a `submission.toml` error names its line; `--max-error-pct` now fails (exit 1) when a row never
  settled; the settle test is one part in a million per second as documented (it was four times looser), so a slow
  rotor settles a little later and closer to its steady state. Without the flag the exit code is unchanged.
- A repro bundle's `journal.txt` must open with its `# ofs-journal 1` header (a byte-order mark is accepted), and
  `ofs.repro.find_bundles` skips a folder that has no `bundle.toml` yet (still being written). `replay_bundle` reads
  `ofs-sim`'s output as UTF-8, so accented paths and Windows error texts come back readable.
#### Fixed
- Stale comments and docs: the cell voltages are the battery model's own, not equal shares.

### M6c: validation and the repro bundle
#### Added
- A failed session writes a **repro bundle**: a folder under the logs directory (`<quad>-<n>.repro`) with the quad,
  the library and world it used, the seed, a journal of everything that steered the run, the firmware's state from
  `Load` and the last 10 seconds of its channels. `ofs-sim --replay <folder>` and `ofs.repro.replay_bundle` run it again
  and say whether the failure came back (`docs/repro-bundle.md`). Every session now keeps that 10 s tail in memory (about 15 MB
  for the shipped quad; it costs about 6 % of the open-loop real-time factor, 275x to 257x) and a journal of its
  steering calls.
- `ofs-validate`, the validation harness: a documented submission format for community measurements (`thrust_stand`,
  and `blackbox_step` as a checked format), a checker that reports every problem with its file and line, and a
  thrust-stand comparator that runs the simulator's own ESC, motor and propeller to steady state at each measured row.
  A synthetic worked example and its generated report are under `validation/`; CI prints the report in the job summary
  (tracked, not gating). See `docs/validation.md`.
- `ofs.repro` (`find_bundles`, `replay_bundle`), and `ofs_config::load_with_library_env`.
#### Changed
- The `sim_error` event's text now ends with `(repro bundle: <folder>)` (or why there is none).
- `ofs-sim --data-dir` has no default shown in `--help` any more (it is still `.ofs-data`); with `--replay` it defaults
  to a temporary directory.

### M6b: battery, thermal and receiver
#### Added
- The video receiver's thresholds are world data: an optional `[video]` table in a world file sets the SNR
  thresholds, field counts and drop probability (`docs/research/video-link.md`). Without it nothing changes.
- A per-cell battery: the pack is a series string of cells with their own resistance, state of charge and capacity;
  battery parts may set a per-cell spread and ageing (`cycles`, capacity fade, resistance growth), the library has an
  aged example pack, and recordings gain `battery.cell.{k}.voltage_v` and `.soc` channels
  (`docs/research/battery-and-thermal.md`).
- A thermal model: one RC node per ESC, motor and VTX, heated by the losses already on the bus, with the ambient
  set by an optional `[environment] ambient_c` in the world file. Parts carry optional `thermal` sections (the library
  parts ship estimated constants), and recordings gain `esc.{i}.temp_c`, `motor.{i}.temp_c` and `vtx.temp_c`
  (`docs/research/battery-and-thermal.md`). Nothing derates.
#### Changed
- The weak-cell fault lands on the cell it names, and the state stream's cell voltages are the battery model's own
  (they used to be equal shares of the pack voltage).
- The recording pattern `battery.*` now also matches the per-cell channels (16 signals on the 6S pack instead of 4); a
  script that counted them, or that relied on `battery.voltage`'s near-miss suggestions, needs a narrower pattern.
- The KISS telemetry frames carry each ESC's simulated temperature and Tramp's status reply the VTX's, instead of a
  constant 25 °C (which is what they still report with no heating, at the default ambient).
- The radio goldens' digests were regenerated for the new recorded channels.

### M6a: rotor and frame physics
#### Added
- Rotor gyroscopic coupling: the spinning rotors' angular momentum now pushes back on a turning airframe
  (`docs/research/rotor-dynamics.md`).
- Windmilling: a propeller's torque now depends on the air coming up through its disc, so an unpowered rotor in a fast
  descent keeps turning. The prop part's new optional `windmill_j0` sets it (0 = off), and recordings gain the
  `prop.{i}.inflow_mps` channel.
- A flight-controller mount: an optional `[frame.fc_mount]` (`natural_hz`, `damping_ratio`) in a frame part passes the
  motors' vibration through a second-order transmissibility before the IMU feels it, and the library part
  `opendrone-5f-softmount` is a soft-mounted example. A frame without it is rigid, as before.
#### Changed
- Flights where the clockwise and counter-clockwise rotors do not stay balanced (a yaw input, a failed or damaged
  motor) differ from 0.1.0; balanced flights, such as the radio goldens' roll and pitch steps, are bit-identical.
- Recordings carry one more channel per motor (`prop.{i}.inflow_mps`), so the radio goldens' digests were regenerated.
  A descent now lowers a prop's torque by the advance-ratio correction (`prop.windmill_j0` on the shipped prop).

### M5: swappable radio components
M5 was specified as a real radio over USB serial; the capture found no CRSF stream on the TX15's port
(`docs/research/serial-radio.md`), so the joystick stays the input and the milestone became the radio's structure and
recording a live flight.
#### Added
- The radio as three swappable parts, `Handset`, `Link` and `Receiver`, stepped by `RadioChain`
  (`crates/ofs-radio`): the `joystick` handset, the `elrs` and `fixed` links and the `crsf` receiver.
- ExpressLRS on 900 MHz: a `band` key (`"2g4"`, the default, or `"900"`) on the `elrs` link, and the quad
  `quads/opendrone-5f-900mhz.toml`. The 900 MHz band's carrier, bandwidth, noise figure and downlink power are
  estimates.
- The `fixed` link (a geometry-free loss fraction, bursts and latency) and its `[radio.fixed]` table, with the quad
  `quads/opendrone-5f-fixed-link.toml`.
- `[radio]` keys `handset` and `receiver` (each defaults to the only variant it has), and a test that swaps a link
  defined inside a test file into a flying chain.
- Recording a live flight's sticks to a `play_inputs` CSV: protocol 10 (`StartInputsRecording`,
  `StopInputsRecording` returning `{path, rows, duration_s}`, the events `inputs_recording_started` and
  `inputs_recording_stopped`), Python `sim.record_inputs()` and `sim.stop_inputs_recording()`, and the game's **I**
  key with a `REC STICKS` indicator. Two lockstep replays of one recording write byte-identical MCAP.
- Golden tests that pin the ELRS link's bytes and a recorded flight's digests, one digest file per OS (the libm
  differs between Windows and Linux).
- `docs/writing-a-radio-model.md` and `docs/research/serial-radio.md`.
#### Changed
- `link_stats_interval_packets` moved from the link to the receiver internally; quad files are unchanged.
- The protocol is version 10: a client built before M5 is refused at the handshake.

### the v1 close-out
#### Fixed
- The first boot's diff apply is judged on the config run's exit and its whole output: a run that failed no longer
  passes because SITL had created `eeprom.bin`, and a good run is no longer refused when its last line was still in
  the pipe.
- A SITL that cannot bind its ports now says which port failed and who held it at the moment of the bind.
- A DVR start on an existing file fails instead of truncating the earlier recording.
- The DVR keys its frames on an integer field index, so a long flight can no longer stretch a recording by a frame;
  the video field rate is defined once (`ofs-core`).
- The goggles' latency is reported without a transmitting VTX.
- The burst-loss validation message states its units (seconds).
- The pre-launch SITL cleanup waits for its victims to exit before the next launch binds (the port-bind flake
  caught by CI); under WSL the wait runs in the VM, and a victim that ignores SIGTERM is SIGKILLed after a grace.
#### Added
- `scripts/benchmark.py`: the real-time factor of headless runs, in CI's job summary (fails below 1×).
- Tests: the ELRS link test runs in CI; a PID saved over MSP persists through a reboot and a new session; a real
  tap overflow counts Betaflight's lost bytes; same-seed lockstep runs with latency and field drops write identical
  recordings; the DVR end to end in the Godot e2e and its JPEG encode; the radio fault cannot move the model's own
  random stream; MSP replies split across reads; the 50 km test reads Betaflight's own RX-loss flag.
- `docs/roadmap.md`, `docs/architecture.md`, `CONTRIBUTING.md`, this changelog.
#### Changed
- The per-milestone carried-debt files moved to `docs/superpowers/history/`; the roadmap replaces them.
- The OSD font atlas's GPL-3.0-only licence is accepted and documented (README, Licence).

## 0.1.0 — 2026-10-13 (v1)
The first release: milestones M0–M4 of `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.
- **Headless core (M0, M1):** real Betaflight SITL in exact lockstep with 6-DOF physics, battery, ESC and motor,
  IMU and barometer models; bit-identical runs with the same seed.
- **Radio and pilot (M2):** the ExpressLRS/CRSF link with failsafe, real-time pacing, the firmware lifecycle, the
  Betaflight Configurator over TCP, and the Godot pilot client with joystick input and an FPV camera.
- **Video (M3):** Betaflight's own OSD over MSP DisplayPort; SmartAudio and Tramp VTX control; a 5.8 GHz analog
  link model that breaks the picture up with distance, attitude, buildings and interference; solid world objects;
  a geometric ELRS link; the FC's own telemetry home; video latency, field drops and a DVR.
- **Sandbox (M4):** a component library with recorded provenance; the v1 fault catalog (motor, prop, battery cell,
  radio loss and bursts, gyro, VTX interference), injectable at a simulated time; MCAP recording (byte-identical
  across identical lockstep runs), pandas/CSV/Parquet export, scripted stick inputs, Betaflight's Blackbox logs, a
  live Rerun view; the `open-fpv-sim` package on PyPI.
