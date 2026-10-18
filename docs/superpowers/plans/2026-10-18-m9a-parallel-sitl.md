# M9a: Parallel SITLs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Several simulator sessions on Betaflight SITL run side by side on one machine (#15): the second concurrent session gets its own ports, its own firmware directory and a cleanup that cannot touch the first. The first session behaves exactly as today (same ports, same Configurator address `tcp://127.0.0.1:5761`).

**Architecture:** The SITL patch learns `--port-offset N` and adds it to every port it binds (UDP 9001-9004, TCP 5761 and up). The launcher takes a *slot* (0 to 15; offset = slot x 10) by holding a loopback UDP "lock" socket for the session's life, which the OS releases on any exit, and picks the first slot whose lock and motor port are free. Every place that names the ports, kills a stray SITL or looks at a firmware directory becomes slot-aware: the cleanup, the pre-launch wait and the escalation match only processes launched with that slot's `--port-offset`, and slot n > 0 uses its own firmware directory. Slot 0 keeps today's paths.

**Tech Stack:** C (the Betaflight patch, rebuilt in WSL), Rust (`ofs-fc`, `ofs-sim`, `ofs-client`), Python live tests.

**Spec:** none beyond GitHub issue #15 (`gh issue view 15 --repo hugowis/open-fpv-sim`) and the "Design calls" below, which are binding; `docs/research/sitl-interface.md` §6 (WSL networking and the cleanup rationale) is the background.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; Windows `cargo test --workspace --locked` green; WSL `cargo test --workspace --locked --no-fail-fast -- --test-threads=1` green with `OFS_SITL_LAUNCH` set; `cargo +1.85 check` on the changed crates (not `ofs-godot`). **MSRV is 1.85: `File::try_lock` is not available; use the socket lock of Design call 3.**
- No golden digest moves, no protocol change, no recording-format change. A moved golden is a bug.
- **SITL rebuild:** the patch changes (`third_party/betaflight/ofs-sitl.patch`); rebuild in WSL with `bash scripts/build-sitl.sh` and re-run the live tests against it. The patch file must stay LF (`.gitattributes` enforces it); check with `git ls-files --eol`.
- Line endings: never rewrite a file through Python text mode on Windows (CRLF); use Edit/Write. Worktree-isolated sessions refuse heredocs and compound git commands: use Edit/Write; WSL through PowerShell with a script file in the scratchpad (`wsl -d Ubuntu -- bash <path>`, `$HOME/.cargo/bin` on PATH, Python `~/ofs/venv/bin/python`, install packages with `uv pip install --python ~/ofs/venv/bin/python`).
- Live SITL (WSL): `CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf`; `pkill -9 -x betaflight_SITL` first if a port is busy (this kills every SITL, which is exactly what the new design stops the product from doing).
- Commit trailers: a blank line, then `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` on its own line.

## Design calls (decided here; the executor ledgers any change)

1. **`--port-offset N` in the patch.** An integer 0 to 1000 added to `PORT_PWM_RAW`, `PORT_PWM`, `PORT_STATE`, `PORT_RC` (sitl.c) and to `BASE_PORT` (serial_tcp.c), default 0; `--help` lists it; an invalid value is a startup error. It is a command-line argument, not an environment variable, because environment variables do not cross `wsl.exe` without `WSLENV`.
2. **Slots.** `MAX_SLOTS = 16`, offset = slot x 10 (UDP 9001+off to 9004+off, TCP 5761+off to 5768+off for UARTs 1 to 8: no overlap between slots). Slot 0 is today's ports.
3. **The slot lock.** `SitlSlot::acquire(bind_ip) -> Result<SitlSlot, FcError>` walks the slots in order. For each it binds a UDP socket on `127.0.0.1:47000+slot` (the lock; a bind failure means the slot is taken, by another session or by anything else), then binds the motor socket on `bind_ip:9002+offset` (the existing `rx`; a failure there means a foreign process holds the slot's ports: release the lock and try the next slot). The first success wins and the `SitlSlot` owns both sockets. All 16 failing is `FcError::PortInUse` naming the first slot's motor port. Sockets are released by the OS when the process exits, so a crashed simulator frees its slot. Two simulator processes on the same OS share the lock domain; a Windows simulator and a WSL-native simulator launching into the same VM do not, and the bind-failure check is their backstop (documented, not solved).
4. **Scoped cleanup.** Everything that kills or waits for stray SITLs matches only processes of the slot: the default cleanup argv becomes `pkill -f '^[^ ]*betaflight_SITL[^ ]* .*--port-offset <N>( |$)'` (direct exec, never through a shell: the anchor and the exact `argv[0]` shape keep it from matching a shell or `pkill` itself), prefixed with the `wsl.exe ... -e` as today; the WSL wait script takes the pattern as `$1`; the native Linux wait reads `/proc/<pid>/cmdline` and requires `comm == betaflight_SITL` and `--port-offset <N>`; the SIGKILL escalation uses the same pattern. A user-supplied `OFS_SITL_CLEANUP` stays as the user wrote it. `--port-offset <N>` is always passed on launch, also for 0 and also for the diff-applying `--config` run, so an old SITL binary without the option fails loudly ("Unknown argument") instead of running on the wrong ports.
5. **Firmware directory.** Slot 0 uses `firmware_dir(data_dir, quad)` exactly as today; slot n > 0 uses the sibling `<that dir name>-s<n>`. Its EEPROM starts empty, so its first boot applies the diff like any fresh directory (the first boot of a fresh directory differs from later boots; deterministic comparisons warm up first, as the hover test already does). The vehicle exposes `firmware_dir()`; the server's Blackbox copy and repro source use it (they are created after the build now).
6. **What the world sees.** `LoadReply.configurator_address` and `Vehicle::configurator_address()` report `tcp://127.0.0.1:<5761+offset>`; Python's `sim.configurator_address` and the Godot toast already show whatever the reply says. A simulator process is still one session, so parallel sessions are parallel simulator processes (`ofs.launch()` once per session).
7. **Not in this milestone:** one server hosting several sessions, a configurable slot count or lock-port range, locking across Windows and WSL, parallelising the existing Rust live tests (several of them `pkill -x` every SITL by design and keep running one at a time; the new concurrency tests are the proof), and the native-Windows port (#67, next).

## Review Focus

- A session that starts while another runs does not kill, evict or share ports with it, in either start order (task 4, 6).
- A crashed simulator's stray SITL is still cleaned up when its slot is reused, and only that slot's (task 4).
- Slot 0 is byte-for-byte today's behaviour: ports, Configurator address, firmware directory, cleanup effect (tasks 2, 5).
- A foreign process holding 9002 sends a new session to the next slot instead of failing, and all 16 taken is a clear error (task 3).
- A SITL binary built before this milestone is refused with the unknown-argument message, not run on the wrong ports (task 1, 5).
- A repro bundle and a Blackbox copy of a session in slot n > 0 find that slot's directory (task 5).

---

### Task 1: `--port-offset` in the SITL patch

**Files:**
- Modify: `third_party/betaflight/ofs-sitl.patch` (regenerate it from the patched tree: `git diff` in `~/ofs/betaflight` after editing `src/platform/SIMULATOR/sitl.c`, `src/main/drivers/serial_tcp.c` and, if the port defines live there, `udplink.c`/`serial_tcp.h`), `docs/research/sitl-interface.md` (§1 and §6 note)
- Test: `crates/ofs-fc/tests/sitl_live.rs` (new test, `#[ignore]` like its neighbours)

**Interfaces:**
- Produces: `betaflight_SITL.elf --port-offset N`: UDP 9001+N to 9004+N and TCP 5761+N and up.

- [ ] **Step 1: Write the failing live test** `a_sitl_launched_with_a_port_offset_serves_msp_on_the_shifted_port` in `crates/ofs-fc/tests/sitl_live.rs`: launch the real SITL with the test file's existing helpers plus `--port-offset 10` in the launch argv; assert `MspClient::connect(127.0.0.1:5771)` answers `MSP_API_VERSION`, that nothing answers on 5761, and that `sitl.log` has `bind port 5771 for UART1`.
- [ ] **Step 2: Run** it in WSL against the *current* binary — Expected: FAIL (SITL exits with "Unknown argument: --port-offset").
- [ ] **Step 3: Implement** Design call 1 in `~/ofs/betaflight`, regenerate `ofs-sitl.patch` (only the new hunks should differ; keep LF), run `bash scripts/build-sitl.sh` from this worktree (it re-applies the patch file: that proves the file, not the edited tree).
- [ ] **Step 4: Run** the new test and the whole `sitl_live` file for `ofs-fc` and `ofs-sim` (serially) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(sitl): --port-offset shifts every SITL port (#15)`.

### Task 2: Ports as data (`PortMap`)

**Files:**
- Modify: `crates/ofs-fc/src/sitl/codec.rs` (a `PortMap`), `crates/ofs-fc/src/sitl/bridge.rs` (`BridgeConfig`, the rx/tx addresses, `probe_state_port`), `crates/ofs-sim/src/vehicle.rs` (`configurator_address`), tests that name `MSP_TCP_PORT` keep working (the constants stay as the slot-0 values)
- Test: `crates/ofs-fc/tests/` (new `sitl_ports.rs` or the existing codec tests)

**Interfaces:**
- Produces: `pub struct PortMap { pub offset: u16 }` with `PortMap::slot(slot: u16) -> PortMap` (offset = slot x 10), `pwm_raw()`, `pwm()`, `state()`, `rc()`, `msp()` (each `base + offset`), `Default` = offset 0; `BridgeConfig.ports: PortMap`; `Vehicle::configurator_address()` uses it.

- [ ] **Step 1: Write the failing tests:** `the_default_port_map_is_todays_ports` (9001, 9002, 9003, 9004, 5761), `a_slot_shifts_every_port_by_ten_per_slot` (slot 2: 9021 to 9024 and 5781), `slots_never_share_a_port` (for slots 0 to 15, the UDP set and the TCP range 5761..=5768 shifted are pairwise disjoint), and `a_vehicle_reports_the_configurator_address_of_its_ports` (unit test in `vehicle.rs` or `open_loop_vehicle.rs` using a `PortMap` of slot 3: `tcp://127.0.0.1:5791`; an open-loop vehicle reports none).
- [ ] **Step 2: Run** `cargo test -p ofs-fc -p ofs-sim --lib --tests portmap` (or the names) — Expected: FAIL (types missing).
- [ ] **Step 3: Implement.** Replace every use of the four `PORT_*` constants and `MSP_TCP_PORT` in non-test code by `PortMap` calls (grep `PORT_PWM\|PORT_STATE\|PORT_RC\|MSP_TCP_PORT` under `crates/*/src`); `probe_state_port` probes `ports.state()`; the `PortInUse` error names `ports.pwm()`. Defaults keep all existing behaviour.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast` and clippy — Expected: PASS.
- [ ] **Step 5: Commit** `refactor(fc): the SITL ports are data (#15)`.

### Task 3: Slot allocation

**Files:**
- Create: `crates/ofs-fc/src/sitl/slot.rs`; Modify: `crates/ofs-fc/src/sitl/mod.rs`
- Test: unit tests in `slot.rs`

**Interfaces:**
- Consumes: Task 2's `PortMap`.
- Produces: `pub const MAX_SLOTS: u16 = 16; pub const LOCK_PORT_BASE: u16 = 47000;` `pub struct SitlSlot { pub index: u16, pub ports: PortMap, pub rx: UdpSocket, /* the lock socket, private */ }` with `SitlSlot::acquire(bind_ip: Ipv4Addr) -> Result<SitlSlot, FcError>` and `SitlSlot::acquire_from(bind_ip, first: u16) -> ...` (for tests).

- [ ] **Step 1: Write the failing tests:** `the_first_acquire_takes_slot_zero_and_its_motor_port` (index 0, `rx` bound to 9002 on loopback; skip when something on the machine already holds 9002/47000, as a live SITL does: use ports via `acquire_from` with a high `first` so the test cannot collide); `a_second_acquire_while_the_first_lives_takes_the_next_slot` (indexes differ, `rx` ports differ by 10); `dropping_a_slot_frees_it_for_the_next_acquire`; `a_foreign_holder_of_the_motor_port_skips_the_slot` (bind 9002+offset of the slot to be tried first with a plain `UdpSocket`; the acquire returns the next slot and the first slot's lock is free again afterwards); `a_foreign_holder_of_the_lock_port_skips_the_slot`; `all_slots_taken_is_a_port_in_use_error_naming_a_motor_port` (hold `MAX_SLOTS` slots, the 17th fails with `FcError::PortInUse`).
- [ ] **Step 2: Run** `cargo test -p ofs-fc --lib slot` — Expected: FAIL.
- [ ] **Step 3: Implement** per Design calls 2 and 3. Tests that hold many sockets run with a `first` offset that keeps them away from the real SITL ports; the `acquire_from(bind_ip, first)` variant walks slots `first..first+MAX_SLOTS` for lock and motor ports computed from the slot index so a test can shift the whole range.
- [ ] **Step 4: Run** the tests and clippy — Expected: PASS.
- [ ] **Step 5: Commit** `feat(fc): slots - a machine-wide lock on a set of SITL ports (#15)`.

### Task 4: Slot-scoped cleanup, wait and escalation

**Files:**
- Modify: `crates/ofs-fc/src/sitl/net.rs` (`default_cleanup`, a `victim_pattern(offset) -> String`), `crates/ofs-fc/src/sitl/process.rs` (`WSL_WAIT_SCRIPT` and its caller, `await_cleanup`, `await_cleanup_native`, `live_victims_count`, the escalation `pkill`, the config-run argv), `crates/ofs-fc/tests/sitl_net.rs`, `crates/ofs-fc/tests/sitl_errors.rs` (the tests that name the old cleanup and wait)
- Test: `crates/ofs-fc/tests/sitl_net.rs`, `crates/ofs-fc/tests/sitl_errors.rs` (Linux decoy tests)

**Interfaces:**
- Consumes: Task 2's `PortMap` (the offset).
- Produces: `net::default_cleanup(launch: &[String], offset: u16) -> Vec<String>`; `net::victim_pattern(offset: u16) -> String` = `^[^ ]*betaflight_SITL[^ ]* .*--port-offset <offset>( |$)`; `process::await_cleanup(launch: &[String], offset: u16) -> AwaitCleanup`; `LaunchConfig.port_offset: u16`, appended as `--port-offset N` to every launch and to the config run.

- [ ] **Step 1: Write the failing tests:** `sitl_net.rs`: `the_default_cleanup_matches_only_its_slots_processes` (the argv for offset 10, plain and with a WSL prefix, equals `[..., "pkill", "-f", victim_pattern(10)]`); `the_victim_pattern_matches_a_slot_command_line_and_nothing_else` (regex-match with the `regex` crate if it is already a dev-dependency, else a small hand check through `grep -E` is not allowed: implement the matcher once as `fn is_victim(cmdline_args: &[&str], offset: u16) -> bool` used by the native scan and test the pattern string against it on a table: offset 10 matches `/x/betaflight_SITL.elf --ip 1.2.3.4 --port-offset 10`, not `... --port-offset 100`, not `--port-offset 1`, not `sh -c "betaflight_SITL --port-offset 10"`, not a command line without the option). `sitl_errors.rs` (Linux only): `a_cleanup_for_one_slot_leaves_another_slots_sitl_alive` — copy `sleep` to a temp dir as `betaflight_SITL`, start it twice with `--port-offset 0`-style decoy args (`<copy> 30 --port-offset 0` and `<copy> 30 --port-offset 10`, using an argv0 that `pkill -f` and `comm` both see), run `run_cleanup(default_cleanup(&[], 10))` then `await_cleanup(&[], 10)`: the offset-10 decoy is dead, the offset-0 decoy still alive; and the escalation (a decoy that ignores SIGTERM, e.g. a `sh` trap script running as the renamed binary) is killed for its own slot only. Update the existing tests that call `await_cleanup(&launch)` or `default_cleanup(&launch)` to the new signatures with offset 0 (their expectations about WSL prefixes and the "unix has pkill, plain Windows none" rule stay).
- [ ] **Step 2: Run** `cargo test -p ofs-fc --test sitl_net --test sitl_errors` — Expected: FAIL (compile errors, then the decoy tests).
- [ ] **Step 3: Implement** Design call 4. Keep the wait's reporting (`seen`, `escalated`, `ms`) and the zombie rule. The WSL wait script becomes `pkill -0 -f -- "$1"` and friends with the pattern passed as the script's first argument (`sh -c SCRIPT sh PATTERN`), never interpolated into the script text. The native scan reads `/proc/<pid>/cmdline` (NUL-separated) and uses `is_victim`.
- [ ] **Step 4: Run** the two test files and then, in WSL, the full `ofs-fc` suite incl. live — Expected: PASS.
- [ ] **Step 5: Commit** `fix(fc): the stray-SITL cleanup only touches its own slot (#15)`.

### Task 5: The vehicle takes a slot

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (`build`: acquire the slot for `FcKind::Sitl`, pass `ports`, `port_offset` and the slot to the bridge/launch, the slot's firmware dir, `Vehicle::firmware_dir()`), `crates/ofs-fc/src/sitl/bridge.rs` (the bridge owns the `SitlSlot`; `rx` comes from it), `crates/ofs-sim/src/server.rs` (create the Blackbox copy and read the repro source's firmware dir after the build, from `vehicle.firmware_dir()`), `crates/ofs-sim/src/repro.rs` if it reads the old path
- Test: `crates/ofs-sim/tests/sitl_live.rs`, `crates/ofs-fc/tests/sitl_errors.rs` (the existing "port held" test), unit tests for the directory rule

**Interfaces:**
- Consumes: Tasks 2 to 4.
- Produces: `pub fn slot_firmware_dir(base: &Path, slot: u16) -> PathBuf` (slot 0 returns `base`, slot n the sibling `<name>-s<n>`); `Vehicle::firmware_dir(&self) -> Option<PathBuf>` (SITL only); a bridge that releases its slot when dropped (so a finished session frees it for the next one).

- [ ] **Step 1: Write the failing tests:** unit `slot_zero_keeps_the_firmware_dir_and_other_slots_get_a_sibling` (`slot_firmware_dir(/d/q-ab12, 0) == /d/q-ab12`; slot 3 is `/d/q-ab12-s3`); in `sitl_errors.rs` adapt `a_held_motor_port_is_a_port_in_use_error` (the existing test with `UdpSocket::bind("127.0.0.1:9002")`): with the slot walk a held 9002 now moves the session to slot 1, so assert that instead (the session starts, its `configurator_address` is `tcp://127.0.0.1:5771`) and keep a variant with `BridgeConfig.slots_limit` (or the `acquire_from` range) of 1 that still yields `PortInUse { port: 9002 }`; `sitl_live.rs` (ignored, Linux/WSL) `a_second_session_gets_its_own_slot_and_both_fly`: build two SITL vehicles in two threads at once, assert indexes 0 and 1, configurator addresses 5761 and 5771, distinct firmware dirs, and that each steps 1 simulated second with its own motor commands (different throttle sticks give different `motor_cmd`s); `dropping_the_first_session_lets_the_next_take_slot_zero_again`.
- [ ] **Step 2: Run** them — Expected: FAIL.
- [ ] **Step 3: Implement.** In `build`, for `FcKind::Sitl`, acquire the slot *before* building `LaunchConfig`; `LaunchConfig.workdir = slot_firmware_dir(firmware_dir(...), slot.index)`; the launch argv gets `--port-offset` via the Task 4 field; `default_cleanup` is computed with the slot's offset unless the user gave `OFS_SITL_CLEANUP`/the quad's `cleanup`. The bridge keeps the `SitlSlot` for its whole life, including across Betaflight reboots (the relaunch reuses the same ports). In `server.rs` the order becomes: build the vehicle, then `BlackboxCopy::new(vehicle.firmware_dir()...)` and the repro source's `firmware_dir`.
- [ ] **Step 4: Run** `cargo test --workspace --locked --no-fail-fast`, clippy, and the WSL live files (`ofs-fc`, `ofs-sim`, serially) — Expected: PASS.
- [ ] **Step 5: Commit** `feat(sim): a SITL session takes the first free slot (#15)`.

### Task 6: Two live sessions through the product (Python)

**Files:**
- Create: `python/tests/test_sitl_parallel.py`
- Modify: `.github/workflows/ci.yml` (add the file to the `sitl` job's pytest list)

**Interfaces:**
- Consumes: Task 5.

- [ ] **Step 1: Write the failing test** `test_two_sims_fly_betaflight_side_by_side` (skipped without `OFS_SITL_LAUNCH`): two `ofs.launch()` servers with their own `data_dir`; warm-up load on each (first boot applies the diff) with the first loaded before the second; then both loaded and alive at once; assert the two `sim.configurator_address` differ and the first is `tcp://127.0.0.1:5761`; arm both (`ARM_AND_ANGLE`, as `hover.py`) with different throttle after the arm second, run them alternately for a few simulated seconds, and assert both climb and that session A's altitude is not influenced by B (different throttle, different altitude; neither session reports a firmware restart or an error). A second test `test_closing_one_sim_does_not_disturb_the_other`: close A mid-flight; B keeps stepping and its motor commands stay finite and armed; a third `launch()` afterwards takes the freed slot (its address is 5761 again).
- [ ] **Step 2: Run** it in WSL (script file) — Expected: FAIL before Task 5's code is built into `ofs-sim`, or PASS now; if it passes immediately, run it against the previous commit's binary once to see it fail (the ledger records both).
- [ ] **Step 3: Fix** whatever the live run shows (timing, order); keep the test deterministic (no wall-clock sleeps: lockstep runs).
- [ ] **Step 4: Run** it three times in a row and the other Python SITL files of the CI list — Expected: PASS.
- [ ] **Step 5: Commit** `test: two Betaflight sessions side by side (#15)`.

### Task 7: Docs, changelog, roadmap

**Files:** Modify `docs/dev-setup.md` (Ports section and the live-test note), `docs/research/sitl-interface.md` (§1 build note, §6 cleanup text), `docs/roadmap.md` ("One SITL per machine"), `CHANGELOG.md` ("Unreleased — M9a"), `README.md`/`python/README.md` only if they say one SITL per machine, `CONTRIBUTING.md` (running live tests)

- [ ] **Step 1: Update** the docs: the port table with the offset rule, how slots are taken (lock sockets 47000 to 47015), what is scoped and what is not (`OFS_SITL_CLEANUP` overrides; Windows and WSL simulators do not share locks), the per-slot firmware directory and its first-boot consequence, the rebuild-SITL note and the unknown-argument message of an old binary, that the Rust live tests still run one at a time and why.
- [ ] **Step 2: Check** every claim against the ledger's measurements; `grep -rn "one SITL\|only one SITL\|fixed (UDP\|9001" docs README.md python/README.md CHANGELOG.md CONTRIBUTING.md` outside `docs/superpowers`.
- [ ] **Step 3: Run** the whole gate: Windows `cargo test --workspace --locked --no-fail-fast`, clippy, `cargo +1.85 check`, `PYTHONPATH=python python -m pytest python/tests -q`, and WSL workspace + live + the Python SITL files with the rebuilt SITL — Expected: PASS, goldens unmoved.
- [ ] **Step 4: Commit** `docs: parallel SITL sessions, M9a changelog and roadmap (#15)`.
