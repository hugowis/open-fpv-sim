# M2b — The Godot Pilot Client Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pilot flies the quad from Godot: a grey-box world, an FPV camera, joystick/keyboard capture and a HUD, connected to `ofs-sim` over protocol 2 (`Pilot`, `StreamState`, `Watch`). The client can start the server itself; a pilot vanishing or a second pilot connecting fails safe exactly as the server already enforces; everything is regression-tested headless (engine unit suites, an open-loop end-to-end, and an end-to-end against real Betaflight through the ELRS/CRSF link).

Exit criteria:
- Godot's game scene flies the OpenDrone 5F quad through real Betaflight SITL: armed by sticks, climbed in Angle mode, zero overruns; a reload (Configurator "Save") re-boots Betaflight and re-arms (`godot/tests/e2e_betaflight.gd`).
- With the real server but the open-loop FC, the same scene connects, flies, pauses, cuts and restores the radio, survives a quad reload and a server restart (`godot/tests/e2e_open_loop.gd`, three consecutive passes).
- 144 headless GDScript checks pass (`godot/tests/run_tests.gd`); the Rust side adds 2 proto tests, 18 pure client tests, 11 session tests (8 clean repeats), 2 launch tests, 2 real-binary launch-integration tests and 3 listen-policy tests — `cargo test --workspace --locked` green on Windows and natively on Linux.
- A broken Godot test script can never yield a green run: a parse error exits 1, a runtime error is killed by the runner's `timeout -k` and fails the job (both behaviours verified on 4.7.2).
- `--listen` on a non-loopback address is refused without `--allow-remote` (the last M1 carried-debt item that blocks exposing the server to the pilot client).

**Architecture:**
- **`ofs-proto`.** The generated protocol messages move into their own crate, so the Godot extension does not pull the whole simulator in just to get the message types. `ofs-sim` re-exports them under its old paths.
- **`ofs-client`.** The gRPC client as a plain Rust crate with no Godot types: a supervisor task on its own threads finds (or starts) the server, loads the quad in real time, opens the event stream and the pilot link, and reloads on command — behind a `Client` handle whose `poll`/`pose`/`telemetry`/`set_sticks` never block the game loop. Pure modules (typed errors from `ofs-error-kind`, the data model, the NED→Godot frame conversion, a pose interpolator that absorbs arrival jitter) are tested against an in-process server.
- **`ofs-godot`.** A thin GDExtension (godot-rust 0.5.5) exposing `OfsClient`, a Node that owns a `Client`, converts types at the boundary and emits signals the scene connects to. This one crate is isolated at rust-version 1.94; the rest of the workspace stays at 1.85.
- **The Godot project.** GDScript does what the engine is good at: input mapping (bindings persisted as JSON), settings (defaults < config file < environment < CLI flags), the grey-box world, the drone with spinning props, the FPV and chase cameras with the lens shader, the HUD with a live sticks display, the controls menu, and `app.gd` tying it together. Headless tests run through a tiny assertion helper and `run_tests.gd`; two e2e SceneTree scripts; a screenshot script for human review.
- **CI.** `scripts/run-godot-tests.sh` downloads Godot 4.7.2 (SHA-512 verified), builds anything missing, and runs the unit suites and the open-loop e2e under `timeout -k`; a new `godot` CI job runs it; the msrv job skips `ofs-godot`.

**Tech Stack:**
- Rust stable; workspace `rust-version = "1.85"`, `crates/ofs-godot` alone at `rust-version = "1.94"` (godot-rust 0.5.5 requires it);
- Godot 4.7.2 stable (official release zip, verified against the release's `SHA512-SUMS.txt`), godot-rust (`godot` crate) 0.5 with its default API (4.6), which is forward-compatible with the 4.7.2 runtime;
- tonic 0.12 / prost 0.13 / tokio 1 — the existing workspace stack;
- Python ≥ 3.10 (only for the `errors.py` mapping and the existing tests).

**Spec:** `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md` — §4.3 (the clients), §5.1 (real-time mode and overruns), §7 (error handling), §9 (M2 row: "Fly with a joystick in Godot (grey-box world, FPV camera) through the ELRS/CRSF stack; failsafe works; Configurator connects").
**Server side:** the M2a plan (`docs/superpowers/plans/2026-10-05-m2a-radio-realtime.md`) built everything this client talks to; protocol 2 semantics (sessions, pilot link, watchers, keep-alive, failsafe) are documented there.
**Read before Task 9:** `docs/dev-setup.md` (the `OFS_SITL_LAUNCH` setup that the Betaflight e2e and the live tests need).
**Carried debt:** `docs/superpowers/m1-carried-debt.md` — the non-loopback `--listen` item is resolved here (Task 4). `.superpowers/sdd/2026-10-05-m2a-radio-realtime/progress.md` — the `pilot_busy` error kind is unmapped in Python and M2b introduces a second pilot-bearing client; resolved here (Task 11).

## Decisions made while planning (verified in a scratch build before writing this plan)

1. **The extension must not pull the simulator.** `ofs-godot` links `ofs-client` and `ofs-proto` only; `ofs-sim` keeps building exactly as before, with its `pb` re-exported from `ofs-proto` (`PROTOCOL_VERSION` moves with it).
   - Verified: `cargo test --workspace --locked` green after the split; the binary still refuses remote listeners (Task 4).
2. **godot-rust 0.5.5 with Godot 4.7.2, and the MSRV conflict isolated.** The `godot` crate requires Rust 1.94 while the workspace pins 1.85. `crates/ofs-godot` therefore declares `rust-version = "1.94"` and the msrv CI job checks the workspace with `--exclude ofs-godot`. `cargo clippy -- -W clippy::incompatible_msrv` with `clippy.toml` `msrv = "1.85"` found no API usage above 1.85 in the new crates, so the isolation is bookkeeping that keeps old toolchains useful, not a real fork.
   - Verified: the extension builds as a ~7 MB DLL on Windows, loads in Godot 4.7.2 headless ("Initialize godot-rust (API v4.6.stable.official, runtime v4.7.2.stable.official, safeguards strict)"), and builds as `libofs_godot.so` natively on Linux (WSL Ubuntu), with the workspace tests passing there too.
3. **The client is a supervisor, not a library of awaited calls.** Godot's main loop cannot await; `Client` runs the supervisor on its own tokio runtime and threads and exposes non-blocking `poll()`, `pose()`, `telemetry()`, `set_sticks()`, `command()`.
   - Verified: 11 session tests against a real in-process server, repeated 8 times clean; the open-loop and Betaflight e2e fly the game scene through it.
4. **An abrupt connection loss maps to `Unavailable`.** A server killed mid-session surfaces as a tonic `Unknown` ("h2 protocol error"); the client maps transport failures to `ErrorKind::Unavailable` so the HUD can say "server gone" instead of reporting an internal error.
   - Verified: `a_server_that_goes_away_fails_the_client_as_unavailable`.
5. **Poses are interpolated at display rate.** State messages arrive with jitter; the `StateBuffer` renders by simulated time, holds the newest pose during a stall, drops samples from a restarted session, ignores non-finite input and renormalises attitudes.
   - Verified: 8 interpolation tests, including the paused session that re-learns the clock.
6. **The client may start the server.** Settings name a server binary (default: the checkout's `target/debug/ofs-sim`); `LaunchSpec` starts it, waits for the port, and an already-running server is adopted and left running.
   - Verified: `the_client_launches_the_server_flies_and_stops_it` and `an_already_running_server_is_used_and_left_running` (real binary, real port).
7. **Non-loopback binds are refused without `--allow-remote`** — the M1 carried-debt item: `Load` reads any file on the host and runs the quad file's `fc.launch` argv, so a server other machines can reach is a remote-program-execution feature and must be explicit.
   - Verified: 3 listen-policy tests, including the binary refusing to start (exit 2).
8. **Input mapping and settings live in GDScript with persisted user config.** Channel bindings (roll, pitch, yaw, throttle, aux1–aux4) map axes, buttons and keys, saved to `user://ofs_client_controls.json`; settings resolve defaults < `[client]` in `user://ofs_client.cfg` < `OFS_*` environment variables < command-line flags after `--`.
   - Verified: 76 + 20 headless checks with a fake `Input` singleton.
9. **Visuals were checked with real screenshots**, not imagination: the grey-box world (metre-grid ground, launch pad, pylons, gates, buildings), the drone with spinning props at the reported pose, the FPV camera with its barrel-distortion/exposure/vignette lens, the HUD layout, the sticks display, the controls menu and the help overlay all rendered at 1280×720 and were reviewed.
   - Verified: `godot/tests/shots.gd` produced `shots/*.png` from the real renderer (Windows, GPU).
10. **Godot test scripts can hang the engine.** A script with a parse error exits 1; a script that raises a runtime error (or never quits) leaves the engine running forever. Every headless Godot invocation is wrapped in `timeout -k`, and a kill fails the run.
    - Verified on 4.7.2: a parse-error script exits 1; a runtime-error script is killed by `timeout` (rc 124); the runner script passes the unit suites and the open-loop e2e.
11. **Line endings are pinned for Godot text files.** A CR inside a multi-line GDScript string renders as an extra line break (hit while writing the HUD help text on Windows); `.gitattributes` forces LF for `*.gd`, `*.gdshader`, `*.tscn`, `*.gdextension` and `godot/project.godot`.
12. **Godot-generated `*.uid` files are committed** alongside the scripts they name (each is a stable one-line `uid://…`); `godot/.godot/` is git-ignored.

## Global Constraints

- **License:** GPL-3.0-or-later (code); every new crate's `Cargo.toml` uses `license.workspace = true`.
- **Platforms:** Windows and Linux. Rust tests run on both (live SITL via WSL on Windows). Godot headless tests run in CI on Linux; screenshots need a GPU and are a human step.
- **Toolchains:** the workspace stays `rust-version = "1.85"`; only `crates/ofs-godot` is 1.94. `cargo test --workspace --locked` on stable covers every crate including `ofs-godot`.
- **Godot's target directory:** `godot/ofs.gdextension` points at `res://../target/{debug,release}/…`, i.e. the workspace's default `target/` at the repo root. Do not set `CARGO_TARGET_DIR` elsewhere when running the Godot tests, or the extension will not be found.
- **Frames and units:** the simulator reports world NED / body FRD; Godot is Y-up with forward −Z. The conversion lives in one place (`ofs_client::frames`) and its tests pin every axis; nothing else converts frames.
- **Real time:** the pilot loads the quad in real-time mode (protocol 2); overruns surface as events on the HUD (`warn`/`slow` policies from M2a).
- **Fail-safe behaviour is server-owned.** The client never "helps": a vanished pilot lets the transmitter go off, a reload restarts the session, and the client only reflects phases, events and errors.
- **gRPC:** `ofs-error-kind` metadata values stay `config`, `firmware`, `numerical`, `protocol`, `not_loaded`, `invalid_argument`, `invalid_state`, `pilot_busy`, `internal`. The Rust client maps all nine (Task 2); Python maps the last two in Task 11.
- **Not in M2b:** the OSD/VTX video chain and analog degradation (M3); the sandbox, MCAP/Rerun and the fault catalog (M4); radio path-loss modelling; export templates or packaged builds; remote/multi-player servers (the client uses loopback).
- **Carried-forward minors** stay where they are: server-side stream minors in `.superpowers/sdd/2026-10-05-m2a-radio-realtime/progress.md`, bridge/model minors in `docs/superpowers/m1-carried-debt.md`.

## Review Focus

- **A false-green CI is worse than no CI.** Godot hangs on a runtime script error; the runner's `timeout -k` plus exit-code handling is the guard. Reviewer: try to imagine a test script that passes silently while broken — parse error → exit 1 ✓, runtime error → killed ✓, failed checks → `quit(1)` from `run_tests.gd` ✓.
- **The NED→Godot frame conversion.** A sign slip flips controls or the camera silently. The frames tests pin every axis; the e2e climbs, and the screenshots show the horizon where it belongs.
- **The pilot slot must not leak.** A client killed without `close()` must not hold the pilot slot: the server turns the transmitter off and the slot frees. Tests: `dropping_the_client_turns_the_transmitter_off`, `a_second_pilot_is_refused_while_the_client_flies`, and the e2e disconnect checks.
- **MSRV isolation.** Only `crates/ofs-godot` may declare 1.94; the msrv job must exclude it, and everything else must stay 1.85-compatible.
- **The non-loopback refusal** must not break dev flows: loopback stays free, `--allow-remote` is the explicit door, and the warning prints whenever the bind is not loopback.

## Known intermittents to watch while executing

- **First live run after a SITL rebuild** timed out on its first exchange once (M2a ledger); `e2e_betaflight.gd` can hit it. On failure, capture the full output, check the SITL log, and debug with superpowers:systematic-debugging before completing the task. Do not retry until it passes.
- **Windows Firewall** blocks freshly built executables that talk to Betaflight in WSL ("no motor output within 5000 ms"). Build with a scratch `CARGO_TARGET_DIR` or run the live work WSL-native (commands in `docs/dev-setup.md`).
- **A reboot-path failure in full-suite runs** was seen once in ~16 runs (M2a). `e2e_betaflight.gd` reboots Betaflight on reload; same guidance.

## File Structure

```
Cargo.toml                                   + ofs-proto, ofs-client in workspace.dependencies
crates/ofs-proto/                            NEW: build.rs, src/lib.rs, tests/messages.rs
                                             protocol 2 types shared by the server and its Rust clients
crates/ofs-client/                           NEW crate, no Godot types:
  src/error.rs                               ClientError/ErrorKind from ofs-error-kind + transport mapping
  src/model.rs                               Sticks/Telemetry/Event/Phase/Update/Command/Settings + pb conversions
  src/frames.rs                              NED → Godot (DVec3/DQuat); the single frame conversion
  src/interp.rs                              StateBuffer: display-rate pose interpolation
  src/launch.rs                              LaunchSpec: start/stop the ofs-sim process
  src/worker.rs                              the supervisor: connect, load, streams, reload, shutdown
  src/client.rs                              Client: the pollable handle over the supervisor
  tests/{model,frames,interp}.rs             pure tests
  tests/{common/mod,session,launch}.rs       in-process server + session/launch tests
crates/ofs-godot/                            NEW crate (rust-version 1.94): src/lib.rs — the OfsClient class
crates/ofs-sim/src/listen.rs                 NEW: the listen policy
crates/ofs-sim/src/{lib,main,server}.rs      pb re-export; --allow-remote
crates/ofs-sim/tests/{listen_policy,client_launch}.rs   NEW tests
godot/
  project.godot, ofs.gdextension             the project and the extension manifest
  scripts/{app,controls,settings}.gd         controller, input mapping, settings
  world/{world.gd,grid.gdshader}             the grey-box world
  drone/{drone,fpv_camera,chase_camera}.gd   the drone and its cameras
  ui/{lens.gdshader,lens,sticks_view,hud,controls_menu}.gd
  tests/{testing,run_tests,test_controls,test_settings,test_hud,test_controls_menu}.gd
  tests/{e2e_open_loop,e2e_betaflight,shots}.gd
scripts/run-godot-tests.sh                   NEW: the headless Godot test runner (CI and dev)
.github/workflows/ci.yml                     + godot job; msrv excludes ofs-godot
python/ofs/errors.py, python/ofs/__init__.py + PilotBusy, InternalError
python/tests/test_client.py                  + the mapping test
README.md, docs/dev-setup.md                 the client: install, run, controls, settings
```

Godot generates one `*.uid` file per script, scene and shader: commit them (each is a stable one-line `uid://…` file).

---

### Task 1: The protocol gets its own crate (`ofs-proto`)

**Files:**
- Create: `crates/ofs-proto/Cargo.toml`, `crates/ofs-proto/build.rs`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-proto/tests/messages.rs`
- Modify: `Cargo.toml` (one line), `crates/ofs-sim/Cargo.toml`, `crates/ofs-sim/src/lib.rs`, `crates/ofs-sim/src/server.rs` (one line), `Cargo.lock` (regenerated)

**Interfaces:**
- Produces: `ofs_proto::pb` (the generated `ofs.v1` module) and `ofs_proto::PROTOCOL_VERSION`.
- Preserves: `ofs_sim::pb` as a re-export, so server code and tests keep their paths.

- [ ] **Step 1: Create the crate.** The Godot extension must not pull the whole simulator in just to get the message types; `ofs-sim` and the future `ofs-client` build against this one crate instead.

`crates/ofs-proto/Cargo.toml` — exactly:

```toml
[package]
name = "ofs-proto"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Protocol 2 messages and gRPC stubs shared by the simulator server and its Rust clients"

[dependencies]
tonic.workspace = true
prost.workspace = true

[build-dependencies]
tonic-build.workspace = true
protoc-bin-vendored.workspace = true
```

`crates/ofs-proto/build.rs` — exactly:

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?);
    tonic_build::configure().compile_protos(&["../../proto/ofs/v1/sim.proto"], &["../../proto"])?;
    println!("cargo:rerun-if-changed=../../proto/ofs/v1/sim.proto");
    Ok(())
}
```

`crates/ofs-proto/src/lib.rs` — exactly:

```rust
//! Protocol 2 messages and gRPC stubs, generated from `proto/ofs/v1/sim.proto`. The simulator server
//! (`ofs-sim`) and every Rust client (`ofs-client`, the Godot extension) build against this one crate.

pub mod pb {
    tonic::include_proto!("ofs.v1");
}

/// The protocol version this build speaks; the `Handshake` RPC compares it on both sides.
pub const PROTOCOL_VERSION: u32 = 2;
```

`crates/ofs-proto/tests/messages.rs` — exactly:

```rust
use ofs_proto::pb::{PilotInput, Sticks};
use ofs_proto::PROTOCOL_VERSION;
use prost::Message;

#[test]
fn the_protocol_version_matches_the_python_client() {
    // python/ofs/client.py PROTOCOL_VERSION must equal this; bump both together.
    assert_eq!(PROTOCOL_VERSION, 2);
    let python = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../python/ofs/client.py")).unwrap();
    assert!(python.contains(&format!("PROTOCOL_VERSION = {PROTOCOL_VERSION}")), "python client speaks another protocol");
}

#[test]
fn a_pilot_input_survives_an_encode_decode_round_trip() {
    let input = PilotInput {
        sticks: Some(Sticks { roll: 0.25, pitch: -0.5, yaw: 1.0, throttle: 0.75, aux: vec![1.0, -1.0] }),
        state_rate_hz: 120,
    };
    let bytes = input.encode_to_vec();
    assert_eq!(PilotInput::decode(bytes.as_slice()).unwrap(), input);
}
```

- [ ] **Step 2: Re-point the workspace and the server.** In the root `Cargo.toml`, add after the `ofs-radio = { path = "crates/ofs-radio" }` line:

```toml
ofs-proto = { path = "crates/ofs-proto" }
```

In `crates/ofs-sim/Cargo.toml`: add `ofs-proto.workspace = true` after `ofs-radio.workspace = true`; delete the `prost.workspace = true` line and the whole `[build-dependencies]` section (`tonic-build`, `protoc-bin-vendored` — they belong to `ofs-proto` now). Leave `[dev-dependencies]` untouched in this task. Replace `crates/ofs-sim/src/lib.rs` with exactly:

```rust
//! Open FPV Sim server library: vehicle assembly, sessions, real-time pacing and the gRPC service.
pub mod pacer;
pub mod runner;
pub mod server;
pub mod session;
pub mod streams;
pub mod vehicle;

/// The protocol messages and stubs live in `ofs-proto`; re-exported so server code and tests keep their paths.
pub use ofs_proto::pb;
```

In `crates/ofs-sim/src/server.rs`, change the one line

```rust
pub const PROTOCOL_VERSION: u32 = 2;
```

to

```rust
pub use ofs_proto::PROTOCOL_VERSION;
```

- [ ] **Step 3: Test.** Run `cargo test --workspace` once without `--locked` so Cargo updates `Cargo.lock` for the new crate, then `cargo test --workspace --locked`.

Expected:
- everything compiles; no new warnings in `ofs-sim`;
- the same test binaries pass as before the split, plus `ofs-proto`'s 2 tests (`the_protocol_version_matches_the_python_client`, `a_pilot_input_survives_an_encode_decode_round_trip`);
- `git status` shows only the intended files plus `Cargo.lock`.

- [ ] **Step 4: Commit** (include `Cargo.lock`):

```bash
git add Cargo.toml Cargo.lock crates/ofs-proto crates/ofs-sim
git commit -m "feat(proto): extract the protocol messages into their own ofs-proto crate"
```

---

### Task 2: `ofs-client` — the pure modules (errors, model, frames, interpolation)

**Files:**
- Create: `crates/ofs-client/Cargo.toml`, `crates/ofs-client/src/lib.rs`, `crates/ofs-client/src/error.rs`, `crates/ofs-client/src/model.rs`, `crates/ofs-client/src/frames.rs`, `crates/ofs-client/src/interp.rs`, `crates/ofs-client/tests/model.rs`, `crates/ofs-client/tests/frames.rs`, `crates/ofs-client/tests/interp.rs`
- Modify: `Cargo.lock` (regenerated)

**Interfaces:**
- Consumes: `ofs_proto::pb` (Task 1).
- Produces:
  - `ClientError` / `ErrorKind` with `from_status(&Status)` — the server's `ofs-error-kind` metadata wins, transport failures map to `Unavailable`;
  - `Sticks` (clamped, non-finite → neutral), `Telemetry`, `Event`/`EventKind`, `Phase`, `Update`, `Command`, `Settings`, all with `from_pb`/`to_pb` conversions;
  - `frames`: `pose_ned_to_godot`, the single NED→Godot conversion;
  - `interp::{Sample, StateBuffer}`: pose interpolation by simulated time.

- [ ] **Step 1: Create the crate skeleton.** `crates/ofs-client/Cargo.toml` — exactly:

```toml
[package]
name = "ofs-client"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true
description = "Rust client for the ofs-sim server: session supervisor, pilot link and state interpolation (used by the Godot extension)"

[dependencies]
ofs-proto.workspace = true
glam.workspace = true
thiserror.workspace = true
tonic.workspace = true
tokio.workspace = true
tokio-stream.workspace = true

[dev-dependencies]
ofs-sim = { path = "../ofs-sim" }
proptest.workspace = true
```

`crates/ofs-client/src/lib.rs` — for now only the pure modules (Task 3 adds the rest):

```rust
//! Rust client for the `ofs-sim` server. The Godot extension is a thin layer over it.
pub mod error;
pub mod frames;
pub mod interp;
pub mod model;

pub use error::{ClientError, ErrorKind};
pub use model::*;
```

- [ ] **Step 2: Write the failing tests.** Three suites, exactly:

`crates/ofs-client/tests/model.rs`:

```rust
use ofs_client::{ClientError, ErrorKind, Event, EventKind, Sticks, Telemetry};
use ofs_proto::pb;
use tonic::metadata::MetadataMap;
use tonic::{Code, Status};

fn status(code: Code, kind: Option<&str>, message: &str) -> Status {
    let mut md = MetadataMap::new();
    if let Some(kind) = kind {
        md.insert("ofs-error-kind", kind.parse().unwrap());
    }
    Status::with_metadata(code, message, md)
}

#[test]
fn every_server_error_kind_maps_to_a_typed_error() {
    for (kind, expected) in [
        ("config", ErrorKind::Config),
        ("firmware", ErrorKind::Firmware),
        ("numerical", ErrorKind::Numerical),
        ("protocol", ErrorKind::Protocol),
        ("not_loaded", ErrorKind::NotLoaded),
        ("invalid_argument", ErrorKind::InvalidArgument),
        ("invalid_state", ErrorKind::InvalidState),
        ("pilot_busy", ErrorKind::PilotBusy),
        ("internal", ErrorKind::Internal),
    ] {
        let e = ClientError::from_status(&status(Code::Aborted, Some(kind), "boom"));
        assert_eq!(e.kind, expected, "{kind}");
        assert_eq!(e.kind.as_str(), kind);
        assert_eq!(e.message, "boom");
    }
}

#[test]
fn transport_failures_and_unknown_kinds_have_their_own_mapping() {
    for code in [Code::Unavailable, Code::Unknown, Code::Cancelled, Code::DeadlineExceeded] {
        let e = ClientError::from_status(&status(code, None, "h2 protocol error: error reading a body from connection"));
        assert_eq!(e.kind, ErrorKind::Unavailable, "{code:?}: a lost connection");
    }
    let e = ClientError::from_status(&status(Code::Internal, Some("from_the_future"), "x"));
    assert_eq!(e.kind, ErrorKind::Other);
    assert!(e.message.contains("Internal") && e.message.contains('x'), "{}", e.message);
}

#[test]
fn sticks_are_clamped_and_non_finite_values_become_neutral() {
    let s = Sticks { roll: 2.0, pitch: -3.0, yaw: f64::NAN, throttle: 1.5, aux: [5.0, -5.0, f64::INFINITY, 0.25] }.sanitized();
    assert_eq!((s.roll, s.pitch, s.yaw, s.throttle), (1.0, -1.0, 0.0, 1.0));
    assert_eq!(s.aux, [1.0, -1.0, -1.0, 0.25]);
    let s = Sticks { throttle: -0.5, ..Default::default() }.sanitized();
    assert_eq!(s.throttle, 0.0);
    assert_eq!(Sticks::default().aux, [-1.0; 4], "switches default to off");
}

#[test]
fn telemetry_is_derived_from_a_state_message() {
    let state = pb::State {
        time_s: 12.5,
        position_ned_m: Some(pb::Vec3 { x: 1.0, y: 2.0, z: -30.0 }),
        velocity_ned_mps: Some(pb::Vec3 { x: 3.0, y: 4.0, z: -2.0 }),
        battery_voltage_v: 24.5,
        battery_current_a: 10.0,
        motor_cmd: vec![0.0, 0.055, 0.0, 0.0],
        radio: Some(pb::RadioLink { tx_enabled: true, link_up: true, lq_pct: 98.0, rssi_dbm: -50.0 }),
        running: true,
        overruns: 3,
        fc_restarts: 1,
        ..Default::default()
    };
    let t = Telemetry::from_pb(&state);
    assert_eq!(t.altitude_m, 30.0);
    assert!((t.speed_mps - (9.0f64 + 16.0 + 4.0).sqrt()).abs() < 1e-12);
    assert_eq!(t.climb_mps, 2.0);
    assert!(t.motors_spinning, "5.5 % is armed idle");
    assert!(t.tx_enabled && t.link_up && t.running);
    assert_eq!((t.overruns, t.fc_restarts), (3, 1));
    let disarmed = Telemetry::from_pb(&pb::State { motor_cmd: vec![0.0; 4], ..Default::default() });
    assert!(!disarmed.motors_spinning);
}

#[test]
fn event_kinds_map_and_unknown_kinds_survive() {
    let e = Event::from_pb(pb::Event { time_s: 1.5, kind: pb::EventKind::LinkDown as i32, message: "down".into() });
    assert_eq!((e.kind, e.kind.as_str(), e.time_s, e.message.as_str()), (EventKind::LinkDown, "link_down", 1.5, "down"));
    assert_eq!(Event::from_pb(pb::Event { kind: 999, ..Default::default() }).kind, EventKind::Unknown);
}
```

`crates/ofs-client/tests/frames.rs`:

```rust
use glam::{DQuat, DVec3};
use ofs_client::frames::{quat_to_godot, vec_to_godot};
use proptest::prelude::*;

const FORWARD_GODOT: DVec3 = DVec3::new(0.0, 0.0, -1.0);
const UP_GODOT: DVec3 = DVec3::new(0.0, 1.0, 0.0);
const RIGHT_GODOT: DVec3 = DVec3::new(1.0, 0.0, 0.0);

fn close(a: DVec3, b: DVec3) -> bool {
    (a - b).length() < 1e-12
}

#[test]
fn north_east_down_become_forward_right_down() {
    assert!(close(vec_to_godot(DVec3::new(1.0, 0.0, 0.0)), FORWARD_GODOT), "north is -Z");
    assert!(close(vec_to_godot(DVec3::new(0.0, 1.0, 0.0)), RIGHT_GODOT), "east is +X");
    assert!(close(vec_to_godot(DVec3::new(0.0, 0.0, 1.0)), -UP_GODOT), "down is -Y");
}

#[test]
fn a_level_body_facing_north_has_the_godot_camera_looking_north() {
    let q = quat_to_godot(DQuat::IDENTITY);
    assert!(close(q * FORWARD_GODOT, FORWARD_GODOT));
    assert!(close(q * UP_GODOT, UP_GODOT));
    assert!(close(q * RIGHT_GODOT, RIGHT_GODOT));
}

#[test]
fn yawing_90_degrees_to_the_right_faces_east() {
    // NED yaw is a rotation about +down; positive is clockwise seen from above.
    let q = quat_to_godot(DQuat::from_axis_angle(DVec3::new(0.0, 0.0, 1.0), std::f64::consts::FRAC_PI_2));
    assert!(close(q * FORWARD_GODOT, RIGHT_GODOT), "forward now points east (+X): {:?}", q * FORWARD_GODOT);
    assert!(close(q * UP_GODOT, UP_GODOT));
}

#[test]
fn pitching_up_raises_the_nose() {
    // FRD pitch is a rotation about +right; positive pitches the nose up.
    let q = quat_to_godot(DQuat::from_axis_angle(DVec3::new(0.0, 1.0, 0.0), 0.5));
    let nose = q * FORWARD_GODOT;
    assert!(nose.y > 0.4 && nose.z < 0.0, "{nose:?}");
}

proptest! {
    /// Rotating a body vector and then converting equals converting both and rotating in Godot's frame.
    #[test]
    fn conversion_commutes_with_rotation(
        axis in (-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0).prop_filter("non-zero", |a| a.0 * a.0 + a.1 * a.1 + a.2 * a.2 > 0.01),
        angle in -6.3f64..6.3,
        v in (-10.0f64..10.0, -10.0f64..10.0, -10.0f64..10.0),
    ) {
        let q = DQuat::from_axis_angle(DVec3::new(axis.0, axis.1, axis.2).normalize(), angle);
        let v = DVec3::new(v.0, v.1, v.2);
        let world_then_convert = vec_to_godot(q * v);
        let convert_then_rotate = quat_to_godot(q) * vec_to_godot(v);
        prop_assert!((world_then_convert - convert_then_rotate).length() < 1e-9);
        prop_assert!((quat_to_godot(q).length() - 1.0).abs() < 1e-12);
    }
}
```

`crates/ofs-client/tests/interp.rs`:

```rust
use glam::{DQuat, DVec3};
use ofs_client::interp::{Sample, StateBuffer};

const RATE_HZ: f64 = 240.0;
const LATENCY_S: f64 = 0.010;
/// Arrival jitter pattern (seconds): includes 0, so the smallest delay seen is exactly `LATENCY_S`.
const JITTER_S: [f64; 7] = [0.0, 0.0013, 0.0004, 0.0021, 0.0, 0.0009, 0.0017];
const YAW_RATE: f64 = std::f64::consts::FRAC_PI_2; // rad/s about +Y

fn sample(k: usize) -> Sample {
    let t = k as f64 / RATE_HZ;
    Sample { sim_time_s: t, pos: DVec3::new(10.0 * t, 0.0, 0.0), att: DQuat::from_rotation_y(YAW_RATE * t) }
}

fn arrival(k: usize) -> f64 {
    k as f64 / RATE_HZ + LATENCY_S + JITTER_S[k % JITTER_S.len()]
}

fn filled(delay_s: f64, count: usize) -> StateBuffer {
    let mut b = StateBuffer::new(delay_s);
    for k in 0..count {
        b.push(sample(k), arrival(k), true);
    }
    b
}

#[test]
fn poses_follow_the_simulation_smoothly_despite_arrival_jitter() {
    let delay = 1.0 / RATE_HZ + 0.002;
    let b = filled(delay, 120);
    let latest = 119.0 / RATE_HZ;
    // The display asks at its own rate (144 Hz) over the last 40 ms the buffer can answer.
    let mut now = LATENCY_S + delay + latest - 0.040;
    while now < LATENCY_S + delay + latest {
        let want = now - LATENCY_S - delay;
        let pose = b.pose_at(now).unwrap();
        assert!((pose.pos.x - 10.0 * want).abs() < 1e-9, "x {} vs {}", pose.pos.x, 10.0 * want);
        let yaw_q = DQuat::from_rotation_y(YAW_RATE * want);
        assert!(pose.att.angle_between(yaw_q) < 1e-6, "attitude lags or jumps at now = {now}");
        now += 1.0 / 144.0;
    }
}

#[test]
fn a_stalled_stream_holds_the_newest_pose() {
    let b = filled(0.006, 50);
    let newest = sample(49);
    let pose = b.pose_at(arrival(49) + 5.0).unwrap();
    assert_eq!(pose.pos, newest.pos);
    assert!(pose.att.angle_between(newest.att) < 1e-12);
}

#[test]
fn the_oldest_pose_is_held_before_the_buffer_reaches_back_that_far() {
    let b = filled(0.006, 3);
    assert_eq!(b.pose_at(-100.0).unwrap().pos, sample(0).pos);
}

#[test]
fn there_is_no_pose_before_the_first_sample() {
    assert!(StateBuffer::new(0.006).pose_at(1.0).is_none());
}

#[test]
fn a_restarted_session_drops_the_old_samples() {
    let mut b = filled(0.006, 50);
    let fresh = Sample { sim_time_s: 0.0, pos: DVec3::new(0.0, 5.0, 0.0), att: DQuat::IDENTITY };
    b.push(fresh, arrival(49) + 3.0, true);
    assert_eq!(b.pose_at(arrival(49) + 3.0).unwrap().pos, fresh.pos);
    assert_eq!(b.pose_at(0.0).unwrap().pos, fresh.pos, "no trace of the previous session remains");
}

#[test]
fn a_paused_session_holds_its_pose_and_relearns_the_clock_when_it_runs_again() {
    let delay = 0.006;
    let mut b = filled(delay, 50);
    // Paused: the server keeps sending the same state; simulated time stands still.
    for i in 0..10 {
        b.push(sample(49), arrival(49) + 0.1 + i as f64 * 0.004, false);
    }
    assert_eq!(b.pose_at(arrival(49) + 100.0).unwrap().pos, sample(49).pos);
    // Running again a second later. The new clock offset is the arrival delay of sample 50 onwards.
    let resume_offset = 1.0 + LATENCY_S;
    for k in 50..80 {
        b.push(sample(k), k as f64 / RATE_HZ + resume_offset + JITTER_S[k % JITTER_S.len()], true);
    }
    let now = 70.0 / RATE_HZ + resume_offset + delay;
    let pose = b.pose_at(now).unwrap();
    assert!((pose.pos.x - 10.0 * (70.0 / RATE_HZ)).abs() < 1e-9, "poses follow the new clock: {}", pose.pos.x);
}

#[test]
fn non_finite_samples_are_ignored() {
    let mut b = filled(0.006, 10);
    let bad = Sample { sim_time_s: 1.0, pos: DVec3::new(f64::NAN, 0.0, 0.0), att: DQuat::IDENTITY };
    b.push(bad, 1.0, true);
    let bad = Sample { sim_time_s: 1.0, pos: DVec3::ZERO, att: DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0) };
    b.push(bad, 1.0, true);
    let bad = Sample { sim_time_s: f64::INFINITY, pos: DVec3::ZERO, att: DQuat::IDENTITY };
    b.push(bad, 1.0, true);
    assert_eq!(b.pose_at(5.0).unwrap().pos, sample(9).pos, "the newest good sample is still the newest");
}

#[test]
fn attitudes_are_renormalised() {
    let mut b = StateBuffer::new(0.006);
    b.push(Sample { sim_time_s: 0.0, pos: DVec3::ZERO, att: DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0) }, 0.0, true);
    assert!((b.pose_at(1.0).unwrap().att.length() - 1.0).abs() < 1e-12);
}
```

Run: `cargo test -p ofs-client`.
Expected: compile error — `error.rs`, `model.rs`, `frames.rs`, `interp.rs` do not exist yet.

- [ ] **Step 3: Implement the pure modules.** Four files, exactly:

`crates/ofs-client/src/error.rs`:

```rust
//! Typed client errors, mapped from the server's `ofs-error-kind` metadata (the same kinds the Python client maps).
use tonic::{Code, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Firmware,
    Numerical,
    Protocol,
    NotLoaded,
    InvalidArgument,
    InvalidState,
    PilotBusy,
    Internal,
    /// Nothing answered, or the connection dropped.
    Unavailable,
    /// The server process could not be started or exited during startup.
    Launch,
    Other,
}

impl ErrorKind {
    /// The `ofs-error-kind` string for server kinds; `unavailable`, `launch` and `other` are client-side.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Config => "config",
            ErrorKind::Firmware => "firmware",
            ErrorKind::Numerical => "numerical",
            ErrorKind::Protocol => "protocol",
            ErrorKind::NotLoaded => "not_loaded",
            ErrorKind::InvalidArgument => "invalid_argument",
            ErrorKind::InvalidState => "invalid_state",
            ErrorKind::PilotBusy => "pilot_busy",
            ErrorKind::Internal => "internal",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::Launch => "launch",
            ErrorKind::Other => "other",
        }
    }

    fn from_server(kind: &str) -> Option<ErrorKind> {
        Some(match kind {
            "config" => ErrorKind::Config,
            "firmware" => ErrorKind::Firmware,
            "numerical" => ErrorKind::Numerical,
            "protocol" => ErrorKind::Protocol,
            "not_loaded" => ErrorKind::NotLoaded,
            "invalid_argument" => ErrorKind::InvalidArgument,
            "invalid_state" => ErrorKind::InvalidState,
            "pilot_busy" => ErrorKind::PilotBusy,
            "internal" => ErrorKind::Internal,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct ClientError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ClientError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    /// Maps a gRPC status: the server's `ofs-error-kind` metadata wins. Without it, the connection going away
    /// (which tonic reports as Unavailable, Unknown "h2 protocol error", Cancelled or DeadlineExceeded) is
    /// `Unavailable`; anything else is `Other`.
    pub fn from_status(status: &Status) -> Self {
        let kind = status.metadata().get("ofs-error-kind").and_then(|v| v.to_str().ok()).and_then(ErrorKind::from_server);
        match kind {
            Some(kind) => Self::new(kind, status.message()),
            None if matches!(status.code(), Code::Unavailable | Code::Unknown | Code::Cancelled | Code::DeadlineExceeded) => {
                Self::new(ErrorKind::Unavailable, status.message())
            }
            None => Self::new(ErrorKind::Other, format!("{:?}: {}", status.code(), status.message())),
        }
    }
}
```

`crates/ofs-client/src/model.rs`:

```rust
//! Plain data the client exchanges with its users: sticks in, telemetry/events/phases out.
use std::path::PathBuf;
use std::time::Duration;

use ofs_proto::pb;

use crate::error::{ClientError, ErrorKind};

pub const AUX_COUNT: usize = 4;

/// The transmitter's sticks: roll, pitch, yaw in [-1, 1], throttle in [0, 1], aux switches in [-1, 1]
/// (up to 4; aux 1 arms and aux 2 selects Angle mode in the reference quad's Betaflight config).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; AUX_COUNT] }
    }
}

impl Sticks {
    /// Clamps every value to its range. A non-finite value (which the server would refuse) becomes its neutral
    /// position: 0 for the sticks and throttle, -1 for an aux switch.
    pub fn sanitized(self) -> Sticks {
        fn fix(v: f64, lo: f64, hi: f64, neutral: f64) -> f64 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                neutral
            }
        }
        Sticks {
            roll: fix(self.roll, -1.0, 1.0, 0.0),
            pitch: fix(self.pitch, -1.0, 1.0, 0.0),
            yaw: fix(self.yaw, -1.0, 1.0, 0.0),
            throttle: fix(self.throttle, 0.0, 1.0, 0.0),
            aux: self.aux.map(|a| fix(a, -1.0, 1.0, -1.0)),
        }
    }

    pub(crate) fn to_pb(self) -> pb::Sticks {
        pb::Sticks { roll: self.roll, pitch: self.pitch, yaw: self.yaw, throttle: self.throttle, aux: self.aux.to_vec() }
    }
}

/// Everything the HUD shows, from one state message.
#[derive(Debug, Clone, PartialEq)]
pub struct Telemetry {
    pub time_s: f64,
    pub altitude_m: f64,
    pub speed_mps: f64,
    /// Positive when climbing.
    pub climb_mps: f64,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    /// Motor commands in [0, 1], in Betaflight's motor order.
    pub motor_cmd: Vec<f64>,
    /// True when any motor is commanded above 2 %: armed Betaflight idles its motors at about 5 %.
    pub motors_spinning: bool,
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
    /// The session is paced to the wall clock right now (false while paused).
    pub running: bool,
    pub overruns: u64,
    pub fc_restarts: u32,
    /// Seconds since this state message arrived (filled in when the telemetry is read).
    pub age_s: f64,
}

impl Telemetry {
    pub fn from_pb(s: &pb::State) -> Telemetry {
        let pos = s.position_ned_m.unwrap_or_default();
        let vel = s.velocity_ned_mps.unwrap_or_default();
        let radio = s.radio.unwrap_or_default();
        Telemetry {
            time_s: s.time_s,
            altitude_m: -pos.z,
            speed_mps: (vel.x * vel.x + vel.y * vel.y + vel.z * vel.z).sqrt(),
            climb_mps: -vel.z,
            battery_voltage_v: s.battery_voltage_v,
            battery_current_a: s.battery_current_a,
            motors_spinning: s.motor_cmd.iter().any(|c| *c > 0.02),
            motor_cmd: s.motor_cmd.clone(),
            tx_enabled: radio.tx_enabled,
            link_up: radio.link_up,
            lq_pct: radio.lq_pct,
            rssi_dbm: radio.rssi_dbm,
            running: s.running,
            overruns: s.overruns,
            fc_restarts: s.fc_restarts,
            age_s: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Overrun,
    FirmwareRestarted,
    SimError,
    LinkDown,
    LinkUp,
    PilotConnected,
    PilotDisconnected,
    SessionEnded,
    /// A kind this client does not know (a newer server).
    Unknown,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Overrun => "overrun",
            EventKind::FirmwareRestarted => "firmware_restarted",
            EventKind::SimError => "sim_error",
            EventKind::LinkDown => "link_down",
            EventKind::LinkUp => "link_up",
            EventKind::PilotConnected => "pilot_connected",
            EventKind::PilotDisconnected => "pilot_disconnected",
            EventKind::SessionEnded => "session_ended",
            EventKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Simulated time of the event.
    pub time_s: f64,
    pub kind: EventKind,
    pub message: String,
}

impl Event {
    pub fn from_pb(e: pb::Event) -> Event {
        let kind = match pb::EventKind::try_from(e.kind) {
            Ok(pb::EventKind::Overrun) => EventKind::Overrun,
            Ok(pb::EventKind::FirmwareRestarted) => EventKind::FirmwareRestarted,
            Ok(pb::EventKind::SimError) => EventKind::SimError,
            Ok(pb::EventKind::LinkDown) => EventKind::LinkDown,
            Ok(pb::EventKind::LinkUp) => EventKind::LinkUp,
            Ok(pb::EventKind::PilotConnected) => EventKind::PilotConnected,
            Ok(pb::EventKind::PilotDisconnected) => EventKind::PilotDisconnected,
            Ok(pb::EventKind::SessionEnded) => EventKind::SessionEnded,
            Ok(pb::EventKind::Unspecified) | Err(_) => EventKind::Unknown,
        };
        Event { time_s: e.time_s, kind, message: e.message }
    }
}

/// Where the client is in its connect, load and fly cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Looking for the server (and starting it when allowed).
    Connecting,
    /// The quad is loading; Betaflight takes a few seconds to boot.
    Loading,
    /// The pilot link is open and the simulation runs in real time.
    Flying,
    /// The user paused the simulation.
    Paused,
    /// Something went wrong (see the detail and kind); `Command::Reload` tries again.
    Failed,
    /// The client was shut down.
    Stopped,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Connecting => "connecting",
            Phase::Loading => "loading",
            Phase::Flying => "flying",
            Phase::Paused => "paused",
            Phase::Failed => "failed",
            Phase::Stopped => "stopped",
        }
    }
}

/// What `Client::poll` returns, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Phase { phase: Phase, detail: String, kind: Option<ErrorKind> },
    /// The quad is loaded; `configurator_address` is empty without Betaflight.
    Session { quad_name: String, configurator_address: String },
    Event(Event),
    /// A request failed without ending the session (for example a Pause the server refused).
    Error(ClientError),
}

/// Requests to the supervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    /// Loads the quad again (resetting the drone) or, after a failure, reconnects and retries.
    Reload,
    /// Cuts (true) or restores (false) the radio link: the failsafe test.
    SetRadioLoss(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverrunPolicy {
    /// Catch up in bursts; drop a backlog over 100 ms and count an overrun.
    #[default]
    Warn,
    /// Never burst: simulated time stretches.
    Slow,
}

/// How to start `ofs-sim` when nothing answers on the server address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    /// Passed as `--data-dir`: per-quad Betaflight working directories (EEPROM, SITL log) live here.
    pub data_dir: PathBuf,
    /// Extra environment for the server (for example `OFS_SITL_LAUNCH`); the rest is inherited.
    pub env: Vec<(String, String)>,
    /// The server's stderr is appended here.
    pub log_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// gRPC address of the server, like `127.0.0.1:50051`.
    pub server_addr: String,
    /// Started on `server_addr` when nothing answers there; `None` fails instead.
    pub launch: Option<LaunchSpec>,
    pub quad_path: String,
    pub seed: u64,
    /// Fly without Betaflight: motor commands follow the throttle stick (a testing aid).
    pub open_loop_fc: bool,
    pub overrun_policy: OverrunPolicy,
    /// Vehicle state messages per second, 1..=240.
    pub state_rate_hz: u32,
    /// Stick messages per second.
    pub stick_rate_hz: u32,
    /// How long a launched server may take to start answering.
    pub launch_timeout: Duration,
}

impl Settings {
    pub fn new(quad_path: impl Into<String>) -> Settings {
        Settings {
            server_addr: "127.0.0.1:50051".into(),
            launch: None,
            quad_path: quad_path.into(),
            seed: 1,
            open_loop_fc: false,
            overrun_policy: OverrunPolicy::Warn,
            state_rate_hz: 240,
            stick_rate_hz: 250,
            launch_timeout: Duration::from_secs(15),
        }
    }
}
```

`crates/ofs-client/src/frames.rs`:

```rust
//! Frame conversion between the simulator (world NED, body FRD) and Godot (right-handed, +Y up, the camera
//! looks along -Z, +X to the right).
//!
//! Godot x = East (NED y) / body Right, Godot y = Up (-NED z) / body Up (-FRD z), Godot z = -North / body Back.
//! The same signed permutation `M` converts positions, world vectors and body vectors, and `det M = +1`, so an
//! attitude (a rotation from the body frame to the world frame) converts by conjugation with `M`: its angle is
//! kept and its axis is mapped with `M`.
use glam::{DQuat, DVec3};

/// A position or vector, in NED (world) or FRD (body), as Godot coordinates.
pub fn vec_to_godot(v: DVec3) -> DVec3 {
    DVec3::new(v.y, -v.z, -v.x)
}

/// An attitude (body FRD to world NED) as the rotation of the Godot body frame in the Godot world.
pub fn quat_to_godot(q: DQuat) -> DQuat {
    DQuat::from_xyzw(q.y, -q.z, -q.x, q.w)
}
```

`crates/ofs-client/src/interp.rs`:

```rust
//! Smooth vehicle poses from a jittery state stream.
//!
//! The server sends states at a fixed rate with their simulated time; they reach the client with irregular
//! delays, and the display runs at its own rate. `StateBuffer` keeps the recent samples and answers "where was
//! the vehicle at this moment on the client's clock": it learns the offset between the two clocks (the smallest
//! arrival delay seen lately, since delays only add) and renders a fixed `delay_s` in the past, so the answer is
//! interpolated between two real samples instead of jumping from sample to sample.
use std::collections::VecDeque;

use glam::{DQuat, DVec3};

const MAX_SAMPLES: usize = 64;
/// How many recent arrival offsets the clock estimate looks at (a second at 240 Hz).
const OFFSET_WINDOW: usize = 240;

/// A vehicle pose in Godot's frame at a simulated time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub sim_time_s: f64,
    pub pos: DVec3,
    pub att: DQuat,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub pos: DVec3,
    pub att: DQuat,
}

impl From<Sample> for Pose {
    fn from(s: Sample) -> Pose {
        Pose { pos: s.pos, att: s.att }
    }
}

#[derive(Debug, Clone)]
pub struct StateBuffer {
    samples: VecDeque<Sample>,
    offsets: VecDeque<f64>,
    delay_s: f64,
}

impl StateBuffer {
    /// `delay_s`: how far in the past poses are rendered; about one state period plus a little jitter margin.
    pub fn new(delay_s: f64) -> StateBuffer {
        StateBuffer { samples: VecDeque::new(), offsets: VecDeque::new(), delay_s }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.offsets.clear();
    }

    /// Adds a sample that arrived at `arrival_s` on the client's monotonic clock. `running` is false while the
    /// session is paused: simulated time then stands still, so the clock offset is forgotten until it runs again.
    /// A sample that is not finite is ignored; a sample from before the newest one means the session restarted,
    /// so the old samples are dropped.
    pub fn push(&mut self, mut s: Sample, arrival_s: f64, running: bool) {
        let len = s.att.length();
        if !(s.sim_time_s.is_finite() && s.pos.is_finite() && s.att.is_finite() && arrival_s.is_finite() && len > 1e-9) {
            return;
        }
        s.att = s.att / len;
        if let Some(last) = self.samples.back() {
            if s.sim_time_s < last.sim_time_s {
                self.clear();
            } else if s.sim_time_s == last.sim_time_s {
                if !running {
                    self.offsets.clear();
                }
                return;
            }
        }
        if running {
            self.offsets.push_back(arrival_s - s.sim_time_s);
            if self.offsets.len() > OFFSET_WINDOW {
                self.offsets.pop_front();
            }
        } else {
            self.offsets.clear();
        }
        self.samples.push_back(s);
        if self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// The pose to show at `now_s` on the client's clock. Before the first sample there is none; when the stream
    /// stalls, or while paused, the newest pose is held rather than extrapolated.
    pub fn pose_at(&self, now_s: f64) -> Option<Pose> {
        let latest = *self.samples.back()?;
        let Some(offset) = self.offsets.iter().copied().reduce(f64::min) else {
            return Some(latest.into());
        };
        let target = now_s - offset - self.delay_s;
        if target >= latest.sim_time_s {
            return Some(latest.into());
        }
        let first = *self.samples.front()?;
        if target <= first.sim_time_s {
            return Some(first.into());
        }
        let after = self.samples.partition_point(|s| s.sim_time_s <= target);
        let (a, b) = (self.samples[after - 1], self.samples[after]);
        let t = (target - a.sim_time_s) / (b.sim_time_s - a.sim_time_s);
        Some(Pose { pos: a.pos.lerp(b.pos, t), att: a.att.slerp(b.att, t) })
    }
}
```

- [ ] **Step 4: Test.** Run `cargo test -p ofs-client`.

Expected: 18 tests pass across the three suites and none fail:
- `tests/model.rs`: 5 (`every_server_error_kind_maps_to_a_typed_error`, `transport_failures_and_unknown_kinds_have_their_own_mapping`, `sticks_are_clamped_and_non_finite_values_become_neutral`, `telemetry_is_derived_from_a_state_message`, `event_kinds_map_and_unknown_kinds_survive`);
- `tests/frames.rs`: 5 (the four axis-pinning tests and the quaternion round trip with `proptest`-style cases);
- `tests/interp.rs`: 8 (jitter smoothing, stall hold, early-buffer hold, no pose before the first sample, restart drops old samples, paused holds and re-learns the clock, non-finite ignored, attitudes renormalised).

- [ ] **Step 5: Commit:**

```bash
git add crates/ofs-client Cargo.lock
git commit -m "feat(client): typed errors, the data model, NED-to-Godot frames and pose interpolation"
```
---

### Task 3: `ofs-client` — launching the server and flying a session

**Files:**
- Create: `crates/ofs-client/src/launch.rs`, `crates/ofs-client/src/worker.rs`, `crates/ofs-client/src/client.rs`, `crates/ofs-client/tests/common/mod.rs`, `crates/ofs-client/tests/session.rs`, `crates/ofs-client/tests/launch.rs`, `crates/ofs-sim/tests/client_launch.rs`
- Modify: `crates/ofs-client/src/lib.rs` (the final module list), `Cargo.toml` (one line), `crates/ofs-sim/Cargo.toml` (one line), `Cargo.lock` (regenerated)

**Interfaces:**
- Consumes: `ofs_proto::pb` and the Task 2 pure modules.
- Produces:
  - `launch::{LaunchSpec, LaunchError}`: start the server binary (working dir, `--listen`, `--data-dir`), wait for the port, stop it; a missing program is a launch error with a hint; a server that dies during startup is reported with its status and log;
  - `worker`: the supervisor — connect (starting the server when allowed), load the quad in real time, open the event stream and the pilot link, apply commands (reload, pause/resume, radio cut, quit);
  - `client::Client`: `start(spec) -> Client`, `poll() -> Vec<Update>`, `pose() -> Option<Pose>`, `telemetry() -> Option<Telemetry>`, `set_sticks(Sticks)`, `command(Command)`, `phase()`, `close()` — none of them blocks;
  - `tests/common`: an in-process `ofs-sim` (open-loop sessions, no Betaflight) with helpers.

- [ ] **Step 1: Register the crate.** In the root `Cargo.toml`, add after the `ofs-proto` line:

```toml
ofs-client = { path = "crates/ofs-client" }
```

In `crates/ofs-sim/Cargo.toml`, add to `[dev-dependencies]` (so the integration test can use the real client):

```toml
ofs-client = { path = "../ofs-client" }
```

Replace `crates/ofs-client/src/lib.rs` with exactly:

```rust
//! Rust client for the `ofs-sim` server. The Godot extension is a thin layer over it.
//!
//! `Client::start` launches a supervisor on its own threads: it finds (or starts) the server, loads the quad
//! in real time, opens the event stream and the pilot link, and keeps them alive. A game loop calls `poll` for
//! what happened, `pose` and `telemetry` for what to draw, and `set_sticks` with what the pilot did; none of
//! them blocks.
mod client;
pub mod error;
pub mod frames;
pub mod interp;
pub mod launch;
pub mod model;
mod worker;

pub use client::Client;
pub use error::{ClientError, ErrorKind};
pub use interp::Pose;
pub use model::*;
```

- [ ] **Step 2: Write the failing tests.** Four files, exactly:

`crates/ofs-client/tests/common/mod.rs`:

```rust
//! An in-process `ofs-sim` server (open-loop sessions, no Betaflight) and helpers for driving a `Client` in tests.
#![allow(dead_code)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ofs_client::{Client, EventKind, Phase, Settings, Update};
use ofs_proto::pb::sim_client::SimClient;
use ofs_proto::pb::sim_server::SimServer;
use ofs_proto::pb::{self, Event};
use ofs_sim::server::SimService;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::transport::Channel;

pub const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TestServer {
    pub addr: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> TestServer {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let data_dir = std::env::temp_dir().join(format!("ofs-client-test-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                ready_tx.send(listener.local_addr().unwrap().to_string()).unwrap();
                let service = SimService::new(data_dir);
                let server = tonic::transport::Server::builder()
                    .add_service(SimServer::new(service.clone()))
                    .serve_with_incoming(TcpListenerStream::new(listener));
                tokio::select! {
                    _ = server => {}
                    _ = stop_rx => {}
                }
                service.shutdown();
            });
            runtime.shutdown_background(); // abrupt, like a crashed server: open streams die with the runtime
        });
        TestServer { addr: ready_rx.recv().unwrap(), stop: Some(stop_tx), thread: Some(thread) }
    }

    /// Settings for an open-loop session on this server.
    pub fn settings(&self) -> Settings {
        Settings { server_addr: self.addr.clone(), open_loop_fc: true, ..Settings::new(QUAD) }
    }

    /// Takes the server down at once.
    pub fn kill(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }

    pub fn raw(&self) -> Raw {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let client = runtime.block_on(SimClient::connect(format!("http://{}", self.addr))).unwrap();
        Raw { runtime, client }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A plain gRPC client for checks the `Client` does not expose.
pub struct Raw {
    runtime: tokio::runtime::Runtime,
    pub client: SimClient<Channel>,
}

impl Raw {
    pub fn get_state(&mut self) -> Result<pb::State, tonic::Status> {
        self.runtime.block_on(self.client.get_state(pb::Empty {})).map(|r| r.into_inner())
    }

    pub fn unload(&mut self) {
        self.runtime.block_on(self.client.unload(pb::Empty {})).unwrap();
    }

    /// Opens an event stream (a watcher keeps a session alive when the pilot's client goes away).
    pub fn watch(&mut self) -> tonic::Streaming<Event> {
        self.runtime.block_on(self.client.watch(pb::Empty {})).unwrap().into_inner()
    }

    /// Tries to become the pilot.
    pub fn try_pilot(&mut self) -> Result<(), tonic::Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(2);
        tx.try_send(pb::PilotInput { sticks: Some(pb::Sticks::default()), state_rate_hz: 60 }).unwrap();
        self.runtime.block_on(self.client.pilot(ReceiverStream::new(rx))).map(|_| ())
    }
}

/// A client plus everything it has reported.
pub struct Probe {
    pub client: Client,
    pub log: Vec<Update>,
}

impl Probe {
    pub fn start(settings: Settings) -> Probe {
        Probe { client: Client::start(settings).unwrap(), log: Vec::new() }
    }

    pub fn pump(&mut self) {
        let updates = self.client.poll();
        self.log.extend(updates);
    }

    /// Polls until `condition` holds; panics with the update log when it does not within `timeout`.
    pub fn wait(&mut self, what: &str, timeout: Duration, condition: impl Fn(&Probe) -> bool) {
        let deadline = Instant::now() + timeout;
        loop {
            self.pump();
            if condition(self) {
                return;
            }
            if Instant::now() > deadline {
                panic!("timed out after {timeout:?} waiting for {what}\nphase: {:?}\nlog: {:#?}", self.client.phase(), self.log);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub fn wait_phase(&mut self, phase: Phase, timeout: Duration) {
        self.wait(&format!("phase {phase:?}"), timeout, |p| p.client.phase().0 == phase);
    }

    pub fn event_kinds(&self) -> Vec<EventKind> {
        self.log.iter().filter_map(|u| if let Update::Event(e) = u { Some(e.kind) } else { None }).collect()
    }

    pub fn phases(&self) -> Vec<Phase> {
        self.log.iter().filter_map(|u| if let Update::Phase { phase, .. } = u { Some(*phase) } else { None }).collect()
    }
}

pub const LONG: Duration = Duration::from_secs(15);
pub const SHORT: Duration = Duration::from_secs(5);
```

`crates/ofs-client/tests/session.rs`:

```rust
//! The client against a real (in-process) server: open-loop sessions, so no Betaflight is needed.
mod common;

use std::time::Duration;

use common::*;
use ofs_client::{Command, ErrorKind, EventKind, Phase, Settings, Sticks, Update};

fn flying(server: &TestServer) -> Probe {
    let mut probe = Probe::start(server.settings());
    probe.wait_phase(Phase::Flying, LONG);
    probe
}

fn climb() -> Sticks {
    Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() }
}

#[test]
fn a_client_connects_loads_and_flies() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    assert!(probe.log.iter().any(|u| matches!(u, Update::Session { quad_name, configurator_address }
        if quad_name.contains("OpenDrone") && configurator_address.is_empty())), "{:#?}", probe.log);
    assert_eq!(&probe.phases()[..3], &[Phase::Connecting, Phase::Loading, Phase::Flying]);

    probe.client.set_sticks(climb());
    probe.wait("the drone climbs", LONG, |p| p.client.telemetry().is_some_and(|t| t.altitude_m > 0.5));
    let t = probe.client.telemetry().unwrap();
    assert!(t.tx_enabled && t.link_up && t.running && t.motors_spinning, "{t:?}");
    assert!(t.age_s < 1.0);
    let pose = probe.client.pose().expect("poses follow the states");
    assert!(pose.pos.y > 0.3, "up is +Y in Godot's frame: {pose:?}");
    assert!((pose.pos.y - t.altitude_m).abs() < 0.5, "pose {pose:?} vs altitude {}", t.altitude_m);
}

#[test]
fn a_missing_quad_fails_with_a_config_error() {
    let server = TestServer::start();
    let mut probe = Probe::start(Settings { quad_path: "does/not/exist.toml".into(), ..server.settings() });
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Config));
    assert!(detail.contains("does/not/exist.toml"), "{detail}");
}

#[test]
fn nothing_listening_fails_as_unavailable_with_a_hint() {
    let mut probe = Probe::start(Settings { server_addr: "127.0.0.1:1".into(), ..Settings::new(QUAD) });
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Unavailable));
    assert!(detail.contains("127.0.0.1:1") && detail.contains("cargo run -p ofs-sim"), "{detail}");
}

#[test]
fn pause_and_resume_stop_and_restart_simulated_time() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("states arrive", SHORT, |p| p.client.telemetry().is_some_and(|t| t.running));
    probe.client.send(Command::Pause);
    probe.wait_phase(Phase::Paused, SHORT);
    probe.wait("the session stops running", SHORT, |p| p.client.telemetry().is_some_and(|t| !t.running));
    let paused_at = probe.client.telemetry().unwrap().time_s;
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(probe.client.telemetry().unwrap().time_s, paused_at, "simulated time stands still");
    probe.client.send(Command::Resume);
    probe.wait_phase(Phase::Flying, SHORT);
    probe.wait("time advances again", SHORT, |p| p.client.telemetry().is_some_and(|t| t.running && t.time_s > paused_at + 0.1));
}

#[test]
fn cutting_the_radio_raises_link_events_and_restoring_it_clears_them() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the link is up", SHORT, |p| p.client.telemetry().is_some_and(|t| t.link_up));
    probe.client.send(Command::SetRadioLoss(true));
    probe.wait("link_down", LONG, |p| p.event_kinds().contains(&EventKind::LinkDown));
    probe.wait("the receiver reports the link down", SHORT, |p| p.client.telemetry().is_some_and(|t| !t.link_up));
    probe.client.send(Command::SetRadioLoss(false));
    probe.wait("link_up again", LONG, |p| {
        let kinds = p.event_kinds();
        kinds.iter().rposition(|k| *k == EventKind::LinkUp) > kinds.iter().rposition(|k| *k == EventKind::LinkDown)
    });
}

#[test]
fn dropping_the_client_turns_the_transmitter_off() {
    let server = TestServer::start();
    let mut raw = server.raw();
    let _keeps_the_session_alive = raw.watch(); // a watcher outlives the client, so the session stays loaded
    let mut probe = flying(&server);
    probe.wait("the transmitter is on", SHORT, |p| p.client.telemetry().is_some_and(|t| t.tx_enabled));
    assert!(raw.get_state().unwrap().radio.unwrap().tx_enabled);
    drop(probe);
    let deadline = std::time::Instant::now() + SHORT;
    loop {
        let state = raw.get_state().expect("the watched session survives");
        if !state.radio.unwrap().tx_enabled {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the transmitter stayed on after the client went away");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_second_pilot_is_refused_while_the_client_flies() {
    let server = TestServer::start();
    let _probe = flying(&server);
    let status = server.raw().try_pilot().unwrap_err();
    assert_eq!(ofs_client::ClientError::from_status(&status).kind, ErrorKind::PilotBusy);
}

#[test]
fn reload_restarts_the_session_and_the_pilot_comes_back() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.client.set_sticks(climb());
    probe.wait("some flight time", LONG, |p| p.client.telemetry().is_some_and(|t| t.time_s > 1.0 && t.altitude_m > 0.3));
    let before = probe.client.telemetry().unwrap().time_s;
    let seen = probe.log.len();
    probe.client.send(Command::Reload);
    probe.wait("loading again", SHORT, |p| p.log[seen..].iter().any(|u| matches!(u, Update::Phase { phase: Phase::Loading, .. })));
    probe.wait_phase(Phase::Flying, LONG);
    probe.wait("states of the new session", LONG, |p| p.client.telemetry().is_some_and(|t| t.time_s < before));
    assert!(probe.client.telemetry().unwrap().tx_enabled, "the pilot link was re-established");
}

#[test]
fn a_session_unloaded_underneath_the_client_fails_it_and_a_reload_recovers() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    server.raw().unload();
    probe.wait_phase(Phase::Failed, LONG);
    assert_eq!(probe.client.phase().2, Some(ErrorKind::NotLoaded), "{:?}", probe.client.phase());
    probe.client.send(Command::Reload);
    probe.wait_phase(Phase::Flying, LONG);
}

#[test]
fn a_server_that_goes_away_fails_the_client_as_unavailable() {
    let mut server = TestServer::start();
    let mut probe = flying(&server);
    server.kill();
    probe.wait_phase(Phase::Failed, LONG);
    assert_eq!(probe.client.phase().2, Some(ErrorKind::Unavailable), "{:?}", probe.client.phase());
}

#[test]
fn shutting_down_stops_the_supervisor() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.client.shutdown();
    assert_eq!(probe.client.phase().0, Phase::Stopped);
    probe.client.shutdown(); // idempotent
}
```

`crates/ofs-client/tests/launch.rs`:

```rust
//! Starting a server process, and what the client reports when that goes wrong.
mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::{Probe, QUAD, SHORT};
use ofs_client::launch::ServerProcess;
use ofs_client::{ErrorKind, LaunchSpec, Phase, Settings};

fn spec(program: PathBuf, dir: &std::path::Path) -> LaunchSpec {
    LaunchSpec { program, data_dir: dir.join("data"), env: vec![], log_file: Some(dir.join("ofs-sim.log")) }
}

fn free_port_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

#[test]
fn a_program_that_does_not_exist_is_a_launch_error_with_a_hint() {
    let dir = tempfile_dir("missing");
    let error = ServerProcess::spawn(&spec("definitely-not-ofs-sim".into(), &dir), "127.0.0.1:50999").err().expect("cannot start");
    assert_eq!(error.kind, ErrorKind::Launch);
    assert!(error.message.contains("definitely-not-ofs-sim") && error.message.contains("cargo build -p ofs-sim"), "{}", error.message);
}

#[test]
fn a_server_that_exits_during_startup_is_reported_with_its_status_and_log() {
    // This test binary stands in for a server that refuses its arguments and exits at once.
    let dir = tempfile_dir("exits");
    let settings = Settings {
        server_addr: free_port_addr(),
        launch: Some(spec(std::env::current_exe().unwrap(), &dir)),
        launch_timeout: Duration::from_secs(10),
        ..Settings::new(QUAD)
    };
    let mut probe = Probe::start(settings);
    probe.wait_phase(Phase::Failed, SHORT);
    let (_, detail, kind) = probe.client.phase();
    assert_eq!(kind, Some(ErrorKind::Launch));
    assert!(detail.contains("exited during startup"), "{detail}");
    assert!(detail.contains("--listen") || detail.contains("Unrecognized option"), "the server's own message is included: {detail}");
}

fn tempfile_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ofs-client-launch-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
```

`crates/ofs-sim/tests/client_launch.rs`:

```rust
//! `ofs-client` starting the real `ofs-sim` binary, flying an open-loop session and stopping it again.
use std::net::TcpStream;
use std::time::{Duration, Instant};

use ofs_client::{Client, LaunchSpec, Phase, Settings, Sticks, Update};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

fn free_port_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().to_string()
}

fn wait(client: &Client, what: &str, timeout: Duration, condition: impl Fn(&Client) -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        client.poll();
        if condition(client) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}; phase {:?}", client.phase());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_client_launches_the_server_flies_and_stops_it() {
    let dir = std::env::temp_dir().join(format!("ofs-client-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let addr = free_port_addr();
    let settings = Settings {
        server_addr: addr.clone(),
        launch: Some(LaunchSpec {
            program: env!("CARGO_BIN_EXE_ofs-sim").into(),
            data_dir: dir.join("data"),
            env: vec![],
            log_file: Some(dir.join("ofs-sim.log")),
        }),
        open_loop_fc: true,
        ..Settings::new(QUAD)
    };
    let client = Client::start(settings).unwrap();
    wait(&client, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    client.set_sticks(Sticks { throttle: 0.9, aux: [1.0, -1.0, -1.0, -1.0], ..Default::default() });
    wait(&client, "a climb", Duration::from_secs(15), |c| c.telemetry().is_some_and(|t| t.altitude_m > 0.3));
    assert!(TcpStream::connect(&addr).is_ok(), "the server answers while the client flies");

    drop(client); // unloads the session, then stops the server it started
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(&addr).is_ok() {
        assert!(Instant::now() < deadline, "the launched server was still running 10 s after the client went away");
        std::thread::sleep(Duration::from_millis(50));
    }
    let log = std::fs::read_to_string(dir.join("ofs-sim.log")).unwrap();
    assert!(log.contains("listening on"), "the server's stderr went to the log: {log:?}");
}

#[test]
fn an_already_running_server_is_used_and_left_running() {
    let dir = std::env::temp_dir().join(format!("ofs-client-e2e-attach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let addr = free_port_addr();
    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_ofs-sim"))
        .args(["--listen", &addr, "--data-dir"])
        .arg(dir.join("data"))
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(&addr).is_err() {
        assert!(Instant::now() < deadline, "the server did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
    // A launch spec that would fail if it were used: the client must find the running server first.
    let settings = Settings {
        server_addr: addr.clone(),
        launch: Some(LaunchSpec { program: "no-such-program".into(), data_dir: dir.join("data"), env: vec![], log_file: None }),
        open_loop_fc: true,
        ..Settings::new(QUAD)
    };
    let client = Client::start(settings).unwrap();
    wait(&client, "flying", Duration::from_secs(20), |c| c.phase().0 == Phase::Flying);
    let updates: Vec<Update> = client.poll();
    assert!(updates.iter().all(|u| !matches!(u, Update::Error(_))));
    drop(client);
    assert!(TcpStream::connect(&addr).is_ok(), "a server the client did not start keeps running");
    let _ = server.kill();
    let _ = server.wait();
}
```

Run: `cargo test -p ofs-client --test session`.
Expected: compile error — `launch.rs`, `worker.rs`, `client.rs` do not exist yet.

- [ ] **Step 3: Implement.** Three files, exactly:

`crates/ofs-client/src/launch.rs`:

```rust
//! Starting and stopping the `ofs-sim` server process.
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::error::{ClientError, ErrorKind};
use crate::model::LaunchSpec;

/// A server this client started. Dropping it kills the process; call `Client` shutdown to unload the session
/// first, so Betaflight SITL stops cleanly (killing `ofs-sim` alone orphans SITL on Windows).
pub struct ServerProcess {
    child: Child,
    log_file: Option<std::path::PathBuf>,
}

impl ServerProcess {
    pub fn spawn(spec: &LaunchSpec, listen: &str) -> Result<ServerProcess, ClientError> {
        let mut command = Command::new(&spec.program);
        command.arg("--listen").arg(listen).arg("--data-dir").arg(&spec.data_dir);
        command.envs(spec.env.iter().map(|(k, v)| (k, v)));
        command.stdin(Stdio::null()).stdout(Stdio::null());
        match &spec.log_file {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let log = OpenOptions::new().create(true).append(true).open(path).map_err(|e| {
                    ClientError::new(ErrorKind::Launch, format!("cannot open the server log {}: {e}", path.display()))
                })?;
                command.stderr(Stdio::from(log));
            }
            None => {
                command.stderr(Stdio::null());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console window next to the game
        }
        let child = command.spawn().map_err(|e| {
            ClientError::new(
                ErrorKind::Launch,
                format!(
                    "cannot start {}: {e}. Build the server with `cargo build -p ofs-sim` and point the client at it \
                     (setting `server_bin` or the OFS_SIM_BIN environment variable).",
                    spec.program.display()
                ),
            )
        })?;
        Ok(ServerProcess { child, log_file: spec.log_file.clone() })
    }

    /// The exit status, once the process has ended.
    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// The end of the server's log (at most about 2 kB), for error messages.
    pub fn log_tail(&self) -> String {
        self.log_file.as_deref().map(read_tail).unwrap_or_default()
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_tail(path: &Path) -> String {
    const TAIL_BYTES: u64 = 2000;
    let Ok(mut file) = fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES))).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).trim().to_string()
}
```

`crates/ofs-client/src/worker.rs`:

```rust
//! The supervisor task: connects (starting the server when allowed), loads the quad, opens the event stream and
//! the pilot link, and keeps flying until it is told to reload or quit.
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use ofs_proto::pb::{self, sim_client::SimClient};
use ofs_proto::PROTOCOL_VERSION;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};

use crate::error::{ClientError, ErrorKind};
use crate::frames::{quat_to_godot, vec_to_godot};
use crate::interp::{Sample, StateBuffer};
use crate::launch::ServerProcess;
use crate::model::{Command, Event, OverrunPolicy, Phase, Settings, Sticks, Telemetry, Update};

/// The server frees a disconnected pilot's slot asynchronously, so a pilot that reconnects at once (a reload) can
/// meet `pilot_busy` for a moment.
const PILOT_RETRIES: u32 = 30;
const PILOT_RETRY_DELAY: Duration = Duration::from_millis(100);
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const LAUNCH_POLL: Duration = Duration::from_millis(100);
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(5);

/// State shared between the supervisor and the `Client` handle.
pub(crate) struct Shared {
    pub epoch: Instant,
    pub updates: std::sync::mpsc::Sender<Update>,
    pub sticks: watch::Sender<Sticks>,
    pub buffer: Mutex<StateBuffer>,
    pub telemetry: Mutex<Option<(Telemetry, Instant)>>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    fn phase(&self, phase: Phase, detail: impl Into<String>) {
        let _ = self.updates.send(Update::Phase { phase, detail: detail.into(), kind: None });
    }

    fn failed(&self, error: &ClientError) {
        let _ = self.updates.send(Update::Phase { phase: Phase::Failed, detail: error.message.clone(), kind: Some(error.kind) });
    }

    fn error(&self, error: ClientError) {
        let _ = self.updates.send(Update::Error(error));
    }

    fn reset_state(&self) {
        lock(&self.buffer).clear();
        *lock(&self.telemetry) = None;
    }

    fn ingest(&self, state: pb::State) {
        let arrival_s = self.epoch.elapsed().as_secs_f64();
        if let (Some(p), Some(q)) = (state.position_ned_m, state.attitude) {
            let sample = Sample {
                sim_time_s: state.time_s,
                pos: vec_to_godot(DVec3::new(p.x, p.y, p.z)),
                att: quat_to_godot(DQuat::from_xyzw(q.x, q.y, q.z, q.w)),
            };
            lock(&self.buffer).push(sample, arrival_s, state.running);
        }
        *lock(&self.telemetry) = Some((Telemetry::from_pb(&state), Instant::now()));
    }
}

fn status_error(status: tonic::Status) -> ClientError {
    ClientError::from_status(&status)
}

/// One attempt to reach the server and shake hands with it.
async fn connect_once(addr: &str, timeout: Duration) -> Result<SimClient<Channel>, ClientError> {
    let unavailable = |e: &dyn std::fmt::Display| ClientError::new(ErrorKind::Unavailable, format!("no ofs-sim answers on {addr} ({e})"));
    let endpoint = Endpoint::from_shared(format!("http://{addr}"))
        .map_err(|e| ClientError::new(ErrorKind::Other, format!("bad server address {addr}: {e}")))?
        .connect_timeout(timeout)
        .tcp_nodelay(true);
    let channel = endpoint.connect().await.map_err(|e| unavailable(&e))?;
    let mut client = SimClient::new(channel);
    let hello = client.handshake(pb::HandshakeRequest { protocol_version: PROTOCOL_VERSION });
    let reply = match tokio::time::timeout(timeout, hello).await {
        Err(_) => return Err(unavailable(&"the handshake timed out")),
        Ok(Err(status)) if status.code() == tonic::Code::Unavailable => return Err(unavailable(&status.message())),
        Ok(Err(status)) => return Err(status_error(status)),
        Ok(Ok(reply)) => reply.into_inner(),
    };
    if reply.protocol_version != PROTOCOL_VERSION {
        let message = format!("the server speaks protocol {}, this client {PROTOCOL_VERSION}", reply.protocol_version);
        return Err(ClientError::new(ErrorKind::Protocol, message));
    }
    Ok(client)
}

async fn connect_or_launch(
    settings: &Settings,
    shared: &Shared,
    server: &mut Option<ServerProcess>,
) -> Result<SimClient<Channel>, ClientError> {
    match connect_once(&settings.server_addr, PROBE_TIMEOUT).await {
        Ok(client) => return Ok(client),
        Err(e) if e.kind != ErrorKind::Unavailable => return Err(e),
        Err(e) => {
            if settings.launch.is_none() {
                let hint = "Start it with `cargo run -p ofs-sim`, or let the client launch it (setting `server_bin`).";
                return Err(ClientError::new(ErrorKind::Unavailable, format!("{} {hint}", e.message)));
            }
        }
    }
    let spec = settings.launch.as_ref().expect("checked above");
    shared.phase(Phase::Connecting, format!("starting {}", spec.program.display()));
    *server = None; // a server of ours that died: stop its leftovers before starting the next one
    let mut process = ServerProcess::spawn(spec, &settings.server_addr)?;
    let deadline = Instant::now() + settings.launch_timeout;
    loop {
        if let Some(status) = process.try_wait() {
            let tail = process.log_tail();
            let log = if tail.is_empty() { String::new() } else { format!("\n{tail}") };
            return Err(ClientError::new(
                ErrorKind::Launch,
                format!("{} exited during startup ({status}). Is the port in {} already taken?{log}", spec.program.display(), settings.server_addr),
            ));
        }
        match connect_once(&settings.server_addr, PROBE_TIMEOUT).await {
            Ok(client) => {
                *server = Some(process);
                return Ok(client);
            }
            Err(e) if e.kind == ErrorKind::Unavailable && Instant::now() < deadline => tokio::time::sleep(LAUNCH_POLL).await,
            Err(e) => return Err(e),
        }
    }
}

/// Forwards the server's events as updates until the stream ends.
fn watch_events(mut stream: tonic::Streaming<pb::Event>, shared: Arc<Shared>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Ok(Some(event)) = stream.message().await {
            let _ = shared.updates.send(Update::Event(Event::from_pb(event)));
        }
    })
}

/// The open pilot link: a task sending the latest sticks at a fixed rate and a task receiving states.
struct PilotLink {
    sender: JoinHandle<()>,
    receiver: JoinHandle<Result<(), ClientError>>,
}

impl PilotLink {
    /// Ends both tasks (dropping the streams, which turns the transmitter off) and waits until they are gone.
    async fn stop(self) {
        self.sender.abort();
        let _ = self.sender.await;
        if !self.receiver.is_finished() {
            self.receiver.abort();
            let _ = self.receiver.await;
        }
    }
}

async fn open_pilot(client: &mut SimClient<Channel>, shared: &Arc<Shared>, settings: &Settings) -> Result<PilotLink, ClientError> {
    let period = Duration::from_secs_f64(1.0 / f64::from(settings.stick_rate_hz.max(1)));
    for attempt in 1..=PILOT_RETRIES {
        let (tx, rx) = mpsc::channel::<pb::PilotInput>(8);
        let first = pb::PilotInput { sticks: Some(shared.sticks.borrow().to_pb()), state_rate_hz: settings.state_rate_hz };
        let _ = tx.send(first).await; // queued before the call: the server reads it to set the stream up
        match client.pilot(ReceiverStream::new(rx)).await {
            Ok(response) => {
                let mut stream = response.into_inner();
                let mut sticks = shared.sticks.subscribe();
                let sender = tokio::spawn(async move {
                    let mut tick = tokio::time::interval(period);
                    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    loop {
                        tick.tick().await;
                        let latest = sticks.borrow_and_update().to_pb();
                        if tx.send(pb::PilotInput { sticks: Some(latest), state_rate_hz: 0 }).await.is_err() {
                            break; // the stream closed
                        }
                    }
                });
                let shared = shared.clone();
                let receiver = tokio::spawn(async move {
                    loop {
                        match stream.message().await {
                            Ok(Some(state)) => shared.ingest(state),
                            Ok(None) => return Ok(()),
                            Err(status) => return Err(status_error(status)),
                        }
                    }
                });
                return Ok(PilotLink { sender, receiver });
            }
            Err(status) => {
                let error = status_error(status);
                if error.kind == ErrorKind::PilotBusy && attempt < PILOT_RETRIES {
                    tokio::time::sleep(PILOT_RETRY_DELAY).await;
                    continue;
                }
                return Err(error);
            }
        }
    }
    unreachable!("the last attempt returns")
}

enum Fly {
    Reload,
    Quit,
    Failed(ClientError),
}

/// Loads the quad, starts it, opens the pilot link and serves commands until something ends the flight.
async fn fly(client: &mut SimClient<Channel>, shared: &Arc<Shared>, settings: &Settings, commands: &mut mpsc::UnboundedReceiver<Command>) -> Fly {
    shared.reset_state();
    shared.phase(Phase::Loading, format!("loading {} (Betaflight takes a few seconds to boot)", settings.quad_path));
    let policy = match settings.overrun_policy {
        OverrunPolicy::Warn => pb::OverrunPolicy::Warn,
        OverrunPolicy::Slow => pb::OverrunPolicy::Slow,
    };
    let load = pb::LoadRequest {
        quad_path: settings.quad_path.clone(),
        seed: settings.seed,
        mode: pb::Mode::Realtime as i32,
        open_loop_fc: settings.open_loop_fc,
        overrun_policy: policy as i32,
        keep_alive: false,
    };
    let reply = match client.load(load).await {
        Ok(reply) => reply.into_inner(),
        Err(status) => return Fly::Failed(status_error(status)),
    };
    let _ = shared.updates.send(Update::Session { quad_name: reply.quad_name, configurator_address: reply.configurator_address });
    if let Err(status) = client.start(pb::Empty {}).await {
        return Fly::Failed(status_error(status));
    }
    let mut link = match open_pilot(client, shared, settings).await {
        Ok(link) => link,
        Err(error) => return Fly::Failed(error),
    };
    shared.phase(Phase::Flying, "");
    let outcome = loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => break Fly::Quit,
                Some(Command::Reload) => break Fly::Reload,
                Some(Command::Pause) => match client.pause(pb::Empty {}).await {
                    Ok(_) => shared.phase(Phase::Paused, ""),
                    Err(status) => shared.error(status_error(status)),
                },
                Some(Command::Resume) => match client.start(pb::Empty {}).await {
                    Ok(_) => shared.phase(Phase::Flying, ""),
                    Err(status) => shared.error(status_error(status)),
                },
                Some(Command::SetRadioLoss(on)) => {
                    let result = if on {
                        let fault = pb::Fault { kind: Some(pb::fault::Kind::RadioLinkLoss(pb::RadioLinkLoss {})) };
                        client.inject_fault(fault).await.map(|_| ())
                    } else {
                        client.clear_faults(pb::Empty {}).await.map(|_| ())
                    };
                    if let Err(status) = result {
                        shared.error(status_error(status));
                    }
                }
            },
            ended = &mut link.receiver => {
                break Fly::Failed(match ended {
                    Ok(Ok(())) => ClientError::new(ErrorKind::NotLoaded, "the server closed the pilot link"),
                    Ok(Err(error)) => error,
                    Err(join) => ClientError::new(ErrorKind::Internal, format!("the state receiver stopped: {join}")),
                });
            }
        }
    };
    link.stop().await;
    outcome
}

enum End {
    Quit,
    Failed(ClientError),
}

/// One connection: reach the server, watch its events, then fly (reloading as often as asked).
async fn cycle(
    settings: &Settings,
    shared: &Arc<Shared>,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    server: &mut Option<ServerProcess>,
) -> End {
    shared.phase(Phase::Connecting, format!("connecting to {}", settings.server_addr));
    let mut client = match connect_or_launch(settings, shared, server).await {
        Ok(client) => client,
        Err(error) => return End::Failed(error),
    };
    // Watch first: a session the server sees watched ends when this client goes away.
    let watcher = match client.watch(pb::Empty {}).await {
        Ok(stream) => watch_events(stream.into_inner(), shared.clone()),
        Err(status) => return End::Failed(status_error(status)),
    };
    let end = loop {
        match fly(&mut client, shared, settings, commands).await {
            Fly::Reload => continue,
            Fly::Quit => break End::Quit,
            Fly::Failed(error) => break End::Failed(error),
        }
    };
    if server.is_some() {
        // Unload stops Betaflight SITL cleanly; killing a launched server first would orphan it on Windows.
        let _ = tokio::time::timeout(UNLOAD_TIMEOUT, client.unload(pb::Empty {})).await;
    }
    watcher.abort();
    end
}

/// The supervisor's whole life: cycles until the commands channel closes.
pub(crate) async fn run(settings: Settings, shared: Arc<Shared>, mut commands: mpsc::UnboundedReceiver<Command>) {
    let mut server: Option<ServerProcess> = None;
    loop {
        match cycle(&settings, &shared, &mut commands, &mut server).await {
            End::Quit => break,
            End::Failed(error) => {
                shared.failed(&error);
                // Wait for a reload (reconnect and try again) or for the end.
                loop {
                    match commands.recv().await {
                        Some(Command::Reload) => break,
                        Some(_) => {}
                        None => {
                            drop(server);
                            shared.phase(Phase::Stopped, "");
                            return;
                        }
                    }
                }
            }
        }
    }
    drop(server);
    shared.phase(Phase::Stopped, "");
}
```

`crates/ofs-client/src/client.rs`:

```rust
//! The client handle: a supervisor running on its own threads, polled without blocking from a game loop.
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};

use crate::error::{ClientError, ErrorKind};
use crate::interp::{Pose, StateBuffer};
use crate::model::{Command, Phase, Settings, Sticks, Telemetry, Update};
use crate::worker::{lock, run, Shared};

/// How long `shutdown` waits for the supervisor (it unloads the session of a server it launched).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(10);

/// What `poll` has seen so far, for the accessors.
struct Seen {
    phase: Phase,
    detail: String,
    kind: Option<ErrorKind>,
    configurator_address: String,
    quad_name: String,
}

pub struct Client {
    shared: Arc<Shared>,
    commands: Option<mpsc::UnboundedSender<Command>>,
    updates: Mutex<std_mpsc::Receiver<Update>>,
    finished: Mutex<std_mpsc::Receiver<()>>,
    runtime: Option<tokio::runtime::Runtime>,
    seen: Mutex<Seen>,
}

impl Client {
    /// Starts the supervisor and returns at once; progress arrives through `poll`.
    pub fn start(mut settings: Settings) -> Result<Client, ClientError> {
        settings.state_rate_hz = settings.state_rate_hz.clamp(1, 240);
        settings.stick_rate_hz = settings.stick_rate_hz.max(1);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("ofs-client")
            .enable_all()
            .build()
            .map_err(|e| ClientError::new(ErrorKind::Other, format!("cannot start the client runtime: {e}")))?;
        let (updates_tx, updates_rx) = std_mpsc::channel();
        let (sticks_tx, _) = watch::channel(Sticks::default());
        let delay_s = 1.0 / f64::from(settings.state_rate_hz) + 0.002;
        let shared = Arc::new(Shared {
            epoch: Instant::now(),
            updates: updates_tx,
            sticks: sticks_tx,
            buffer: Mutex::new(StateBuffer::new(delay_s)),
            telemetry: Mutex::new(None),
        });
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (finished_tx, finished_rx) = std_mpsc::channel();
        let worker_shared = shared.clone();
        runtime.spawn(async move {
            run(settings, worker_shared, commands_rx).await;
            let _ = finished_tx.send(());
        });
        Ok(Client {
            shared,
            commands: Some(commands_tx),
            updates: Mutex::new(updates_rx),
            finished: Mutex::new(finished_rx),
            runtime: Some(runtime),
            seen: Mutex::new(Seen {
                phase: Phase::Connecting,
                detail: String::new(),
                kind: None,
                configurator_address: String::new(),
                quad_name: String::new(),
            }),
        })
    }

    /// Replaces the transmitter's sticks (clamped to their ranges); the pilot link sends the latest at its own rate.
    pub fn set_sticks(&self, sticks: Sticks) {
        self.shared.sticks.send_replace(sticks.sanitized());
    }

    pub fn send(&self, command: Command) {
        if let Some(tx) = &self.commands {
            let _ = tx.send(command);
        }
    }

    /// Everything that happened since the last call, oldest first. Never blocks.
    pub fn poll(&self) -> Vec<Update> {
        let updates: Vec<Update> = lock_receiver(&self.updates).try_iter().collect();
        let mut seen = lock(&self.seen);
        for update in &updates {
            match update {
                Update::Phase { phase, detail, kind } => {
                    seen.phase = *phase;
                    seen.detail = detail.clone();
                    seen.kind = *kind;
                }
                Update::Session { quad_name, configurator_address } => {
                    seen.quad_name = quad_name.clone();
                    seen.configurator_address = configurator_address.clone();
                }
                Update::Event(_) | Update::Error(_) => {}
            }
        }
        updates
    }

    /// The phase as of the last `poll`, with its detail and, for `Failed`, the error kind.
    pub fn phase(&self) -> (Phase, String, Option<ErrorKind>) {
        let seen = lock(&self.seen);
        (seen.phase, seen.detail.clone(), seen.kind)
    }

    /// As of the last `poll`: empty without Betaflight, or before the quad is loaded.
    pub fn configurator_address(&self) -> String {
        lock(&self.seen).configurator_address.clone()
    }

    pub fn quad_name(&self) -> String {
        lock(&self.seen).quad_name.clone()
    }

    /// Where to draw the vehicle now (Godot frame), interpolated between the last states received.
    pub fn pose(&self) -> Option<Pose> {
        lock(&self.shared.buffer).pose_at(self.shared.epoch.elapsed().as_secs_f64())
    }

    /// The newest telemetry, with its age.
    pub fn telemetry(&self) -> Option<Telemetry> {
        let (telemetry, received) = lock(&self.shared.telemetry).clone()?;
        Some(Telemetry { age_s: received.elapsed().as_secs_f64(), ..telemetry })
    }

    /// Ends the session (unloading it first when this client launched the server) and stops the supervisor.
    /// Also done on drop.
    pub fn shutdown(&mut self) {
        self.commands = None; // closing the channel is the quit signal
        if let Some(runtime) = self.runtime.take() {
            let _ = lock_receiver(&self.finished).recv_timeout(SHUTDOWN_WAIT);
            runtime.shutdown_timeout(Duration::from_secs(2));
        }
        lock(&self.seen).phase = Phase::Stopped;
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lock_receiver<T>(m: &Mutex<std_mpsc::Receiver<T>>) -> std::sync::MutexGuard<'_, std_mpsc::Receiver<T>> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
```

- [ ] **Step 4: Test.** Run `cargo test -p ofs-client`.

Expected:
- `tests/session.rs`: 11 pass (`a_client_connects_loads_and_flies`, `a_missing_quad_fails_with_a_config_error`, `nothing_listening_fails_as_unavailable_with_a_hint`, `pause_and_resume_stop_and_restart_simulated_time`, `cutting_the_radio_raises_link_events_and_restoring_it_clears_them`, `dropping_the_client_turns_the_transmitter_off`, `a_second_pilot_is_refused_while_the_client_flies`, `reload_restarts_the_session_and_the_pilot_comes_back`, `a_session_unloaded_underneath_the_client_fails_it_and_a_reload_recovers`, `a_server_that_goes_away_fails_the_client_as_unavailable`, `shutting_down_stops_the_supervisor`);
- `tests/launch.rs`: 2 pass (`a_program_that_does_not_exist_is_a_launch_error_with_a_hint`, `a_server_that_exits_during_startup_is_reported_with_its_status_and_log`);
- the Task 2 suites still pass (18).

- [ ] **Step 5: Repeat the session tests eight times** to shake out flakiness:

```bash
for i in 1 2 3 4 5 6 7 8; do cargo test -p ofs-client --test session 2>&1 | grep -E "test result|FAILED|panicked"; done
```

Expected: `test result: ok. 11 passed` eight times, nothing else. (Verified 8/8 during planning.)

- [ ] **Step 6: The real-binary integration test.** Run `cargo test -p ofs-sim --test client_launch`.

Expected: 2 pass — `the_client_launches_the_server_flies_and_stops_it` and `an_already_running_server_is_used_and_left_running`. This starts the actual `ofs-sim` binary on a free port, flies an open-loop session and stops it.

Note (Windows): if a live-style failure mentions the firewall, set `CARGO_TARGET_DIR` to a scratch directory for the run and retry (see Known intermittents).

- [ ] **Step 7: Commit:**

```bash
git add Cargo.toml Cargo.lock crates/ofs-client crates/ofs-sim
git commit -m "feat(client): the session supervisor - server launch, pilot link and a pollable handle"
```

---

### Task 4: The server refuses non-loopback listeners without `--allow-remote`

The M1 carried-debt item, closed before the pilot client spreads: `Load` reads any file on the host and starts the quad file's `fc.launch` program, so a server other machines can reach lets them run programs here.

**Files:**
- Create: `crates/ofs-sim/src/listen.rs`, `crates/ofs-sim/tests/listen_policy.rs`
- Modify: `crates/ofs-sim/src/lib.rs` (one line), `crates/ofs-sim/src/main.rs`

**Interfaces:**
- Produces: `listen::check_listen(addr: SocketAddr, allow_remote: bool) -> Result<(), String>`; the binary gains `--allow-remote` and exits 2 on a refused bind, warning whenever it binds off loopback.

- [ ] **Step 1: Write the failing tests.** `crates/ofs-sim/tests/listen_policy.rs` — exactly:

```rust
use std::net::SocketAddr;
use std::process::Command;

use ofs_sim::listen::check_listen;

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn loopback_addresses_are_always_allowed() {
    for a in ["127.0.0.1:50051", "127.0.0.2:1", "[::1]:50051"] {
        assert!(check_listen(addr(a), false).is_ok(), "{a}");
    }
}

#[test]
fn other_addresses_need_the_explicit_flag() {
    for a in ["0.0.0.0:50051", "192.168.1.20:50051", "[::]:50051", "10.0.0.1:1"] {
        let message = check_listen(addr(a), false).unwrap_err();
        assert!(message.contains("--allow-remote") && message.contains(a), "{message}");
        assert!(check_listen(addr(a), true).is_ok(), "{a} with the flag");
    }
}

#[test]
fn the_binary_refuses_to_start_on_a_remote_address_without_the_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_ofs-sim")).args(["--listen", "0.0.0.0:0"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing to listen on 0.0.0.0:0") && stderr.contains("--allow-remote"), "{stderr}");
}
```

Run: `cargo test -p ofs-sim --test listen_policy`.
Expected: compile error — no `ofs_sim::listen`.

- [ ] **Step 2: Implement.** `crates/ofs-sim/src/listen.rs` — exactly:

```rust
//! Where the server may listen. `Load` reads any file on the host and starts the quad file's `fc.launch` program,
//! so a server that other machines can reach lets them run programs here.
use std::net::SocketAddr;

pub fn check_listen(addr: SocketAddr, allow_remote: bool) -> Result<(), String> {
    if addr.ip().is_loopback() || allow_remote {
        return Ok(());
    }
    Err(format!(
        "refusing to listen on {addr}: it is not a loopback address, and any client that can reach it could make this \
         machine run programs (a quad file's `fc.launch`). Listen on 127.0.0.1, or pass --allow-remote if you trust \
         every machine that can connect."
    ))
}
```

In `crates/ofs-sim/src/lib.rs` add `pub mod listen;` as the first module (the file then matches exactly):

```rust
//! Open FPV Sim server library: vehicle assembly, sessions, real-time pacing and the gRPC service.
pub mod listen;
pub mod pacer;
pub mod runner;
pub mod server;
pub mod session;
pub mod streams;
pub mod vehicle;

/// The protocol messages and stubs live in `ofs-proto`; re-exported so server code and tests keep their paths.
pub use ofs_proto::pb;
```

`crates/ofs-sim/src/main.rs` — exactly (the new `--allow-remote` flag, the refusal, and the warning):

```rust
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::server::SimService;

#[derive(Parser)]
#[command(about = "Open FPV Sim headless server")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:50051")]
    listen: SocketAddr,
    /// Per-quad firmware working directories (EEPROM, SITL log) are created here.
    #[arg(long, default_value = ".ofs-data")]
    data_dir: PathBuf,
    /// Allow listening on a non-loopback address. Any client that can connect can make this machine run programs.
    #[arg(long)]
    allow_remote: bool,
}

/// Ctrl-C, or SIGTERM on unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args = Args::parse();
    if let Err(message) = ofs_sim::listen::check_listen(args.listen, args.allow_remote) {
        eprintln!("ofs-sim: {message}");
        std::process::exit(2);
    }
    if !args.listen.ip().is_loopback() {
        eprintln!("ofs-sim: warning: listening on {} (--allow-remote): every client that can connect can run programs here", args.listen);
    }
    eprintln!("ofs-sim {} listening on {}", env!("CARGO_PKG_VERSION"), args.listen);
    let service = SimService::new(args.data_dir);
    let (signalled, on_signal) = tokio::sync::oneshot::channel::<()>();
    let server = tonic::transport::Server::builder().add_service(SimServer::new(service.clone())).serve_with_shutdown(
        args.listen,
        async move {
            shutdown_signal().await;
            eprintln!("ofs-sim: shutting down");
            let _ = signalled.send(());
        },
    );
    // Open streams (Watch, Pilot, StreamState) would hold a graceful shutdown forever: give them 3 s.
    let grace = async move {
        if on_signal.await.is_ok() {
            tokio::time::sleep(Duration::from_secs(3)).await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    let result = tokio::select! {
        result = server => result.map_err(Into::into),
        _ = grace => {
            eprintln!("ofs-sim: streams still open after 3 s; closing anyway");
            Ok(())
        }
    };
    service.shutdown(); // stops the real-time runner and Betaflight SITL, also when the server failed
    result
}
```

- [ ] **Step 3: Test.** Run `cargo test -p ofs-sim --test listen_policy`.

Expected: 3 pass — `loopback_addresses_are_always_allowed`, `other_addresses_need_the_explicit_flag`, `the_binary_refuses_to_start_on_a_remote_address_without_the_flag`.

Also check by hand: `cargo run -p ofs-sim -- --listen 0.0.0.0:50051` prints the refusal and exits 2; `cargo run -p ofs-sim -- --listen 0.0.0.0:50051 --allow-remote` starts and prints the warning.

- [ ] **Step 4: Commit:**

```bash
git add crates/ofs-sim
git commit -m "feat(sim): refuse non-loopback listeners unless --allow-remote"
```

---

### Task 5: The `ofs-godot` GDExtension and the Godot project skeleton

**Files:**
- Create: `crates/ofs-godot/Cargo.toml`, `crates/ofs-godot/src/lib.rs`, `godot/ofs.gdextension`, `godot/project.godot`
- Modify: `.gitattributes`, `.gitignore`, `Cargo.lock` (regenerated)

**Interfaces:**
- Consumes: `ofs_client::{Client, ...}` (Task 3).
- Produces: the `OfsClient` node class (signals and properties the scene will connect to), the `godot/` project that loads the extension from `res://../target/`.

- [ ] **Step 1: Pin the Godot text files to LF.** A CR inside a multi-line GDScript string renders as an extra line break (hit during verification). Replace `.gitattributes` with exactly:

```
*.sh text eol=lf
*.patch text eol=lf
*.diff text eol=lf
# Godot reads these as LF: a CR inside a multi-line string literal would render as an extra line break.
*.gd text eol=lf
*.gdshader text eol=lf
*.tscn text eol=lf
*.gdextension text eol=lf
godot/project.godot text eol=lf
```

Replace `.gitignore` with exactly (Godot creates `godot/.godot/` on the first import):

```
/target/
/.ofs-data/
/spikes/m0/runs/
/.superpowers/
__pycache__/
*.egg-info/
.venv/
/godot/.godot/
```

- [ ] **Step 2: Create the extension crate.** `crates/ofs-godot/Cargo.toml` — exactly (note the isolated rust-version; the msrv job will skip this crate):

```toml
[package]
name = "ofs-godot"
version.workspace = true
edition.workspace = true
license.workspace = true
# The godot crate (godot-rust) needs a newer Rust than the rest of the workspace; the msrv CI job skips this crate.
rust-version = "1.94"
description = "Godot 4 extension (GDExtension, godot-rust): the pilot client's connection to ofs-sim"

[lib]
crate-type = ["cdylib"]

[dependencies]
ofs-client.workspace = true
godot = "0.5"
```

`crates/ofs-godot/src/lib.rs` — exactly (all logic lives in `ofs-client`; this converts between its plain types and Godot's and drives it from `_process`):

```rust
//! Godot 4 extension: `OfsClient`, a node that connects the pilot's game to `ofs-sim`.
//!
//! All the logic lives in `ofs-client`; this file converts between its plain types and Godot's. The client runs
//! on its own threads and never touches a Godot object: the node polls it once per frame (`process`) and turns
//! what happened into signals, so everything Godot sees happens on the main thread.
use std::path::PathBuf;
use std::time::Duration;

use godot::classes::{INode, Node};
use godot::prelude::*;
use ofs_client::{Client, Command, LaunchSpec, OverrunPolicy, Settings, Sticks, Update};

struct OfsExtension;

#[gdextension]
unsafe impl ExtensionLibrary for OfsExtension {}

/// The pilot's link to the simulator. Call `start(settings)`, feed it `set_sticks` every frame, read `get_pose`
/// and `get_telemetry`, and react to its signals. See `godot/scripts/app.gd` for the whole loop.
#[derive(GodotClass)]
#[class(base = Node)]
pub struct OfsClient {
    base: Base<Node>,
    client: Option<Client>,
}

#[godot_api]
impl INode for OfsClient {
    fn init(base: Base<Node>) -> Self {
        Self { base, client: None }
    }

    fn process(&mut self, _delta: f64) {
        let updates = match &self.client {
            Some(client) => client.poll(),
            None => return,
        };
        for update in updates {
            match update {
                Update::Phase { phase, detail, kind } => {
                    let kind = kind.map(|k| k.as_str()).unwrap_or("");
                    self.signals().phase_changed().emit(&GString::from(phase.as_str()), &GString::from(detail.as_str()), &GString::from(kind));
                }
                Update::Session { quad_name, configurator_address } => {
                    self.signals().session_ready().emit(&GString::from(quad_name.as_str()), &GString::from(configurator_address.as_str()));
                }
                Update::Event(event) => {
                    self.signals().event_received().emit(&GString::from(event.kind.as_str()), &GString::from(event.message.as_str()), event.time_s);
                }
                Update::Error(error) => {
                    self.signals().request_failed().emit(&GString::from(error.kind.as_str()), &GString::from(error.message.as_str()));
                }
            }
        }
    }

    fn exit_tree(&mut self) {
        self.stop();
    }
}

#[godot_api]
impl OfsClient {
    /// The phase changed: "connecting", "loading", "flying", "paused", "failed" or "stopped". `detail` says more
    /// (for "failed", the error message) and `kind` is the error kind for "failed", empty otherwise.
    #[signal]
    fn phase_changed(phase: GString, detail: GString, kind: GString);

    /// The quad is loaded. `configurator_address` (like `tcp://127.0.0.1:5761`) is empty without Betaflight.
    #[signal]
    fn session_ready(quad_name: GString, configurator_address: GString);

    /// A simulator event: "link_down", "link_up", "overrun", "firmware_restarted", "sim_error", ...
    #[signal]
    fn event_received(kind: GString, message: GString, time_s: f64);

    /// A request failed without ending the flight (for example a pause the server refused).
    #[signal]
    fn request_failed(kind: GString, message: GString);

    /// Starts connecting; returns at once. Returns false (and logs why) when already started or the settings are
    /// invalid. Keys: `quad_path` (required), `server_addr`, `server_bin` (a program to start when nothing
    /// answers), `data_dir`, `log_file`, `env` (Dictionary of String to String for the started server), `seed`,
    /// `open_loop`, `overrun_policy` ("warn" or "slow"), `state_rate_hz`, `stick_rate_hz`.
    #[func]
    fn start(&mut self, settings: VarDictionary) -> bool {
        if self.client.is_some() {
            godot_error!("OfsClient.start: already started");
            return false;
        }
        let settings = match parse_settings(&settings) {
            Ok(settings) => settings,
            Err(message) => {
                godot_error!("OfsClient.start: {message}");
                return false;
            }
        };
        match Client::start(settings) {
            Ok(client) => {
                self.client = Some(client);
                true
            }
            Err(error) => {
                godot_error!("OfsClient.start: {error}");
                false
            }
        }
    }

    /// Ends the session and stops a server this node started. Also done when the node leaves the tree.
    #[func]
    fn stop(&mut self) {
        if let Some(mut client) = self.client.take() {
            client.shutdown();
        }
    }

    /// The transmitter's sticks: roll, pitch, yaw in [-1, 1], throttle in [0, 1], up to 4 aux switches in [-1, 1].
    #[func]
    fn set_sticks(&self, roll: f64, pitch: f64, yaw: f64, throttle: f64, aux: PackedFloat32Array) {
        let Some(client) = &self.client else { return };
        let mut sticks = Sticks { roll, pitch, yaw, throttle, ..Sticks::default() };
        for (slot, value) in sticks.aux.iter_mut().zip(aux.as_slice()) {
            *slot = f64::from(*value);
        }
        client.set_sticks(sticks);
    }

    #[func]
    fn pause(&self) {
        self.send(Command::Pause);
    }

    #[func]
    fn resume(&self) {
        self.send(Command::Resume);
    }

    /// Loads the quad again (the drone is back at its start), or retries after a failure.
    #[func]
    fn reload(&self) {
        self.send(Command::Reload);
    }

    /// Cuts (true) or restores (false) the radio link: the failsafe test.
    #[func]
    fn set_radio_loss(&self, on: bool) {
        self.send(Command::SetRadioLoss(on));
    }

    /// The current phase (see `phase_changed`).
    #[func]
    fn get_phase(&self) -> GString {
        GString::from(self.client.as_ref().map_or("stopped", |c| c.phase().0.as_str()))
    }

    #[func]
    fn get_phase_detail(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.phase().1).unwrap_or_default().as_str())
    }

    #[func]
    fn get_configurator_address(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.configurator_address()).unwrap_or_default().as_str())
    }

    #[func]
    fn get_quad_name(&self) -> GString {
        GString::from(self.client.as_ref().map(|c| c.quad_name()).unwrap_or_default().as_str())
    }

    /// True once the first vehicle state has arrived.
    #[func]
    fn has_pose(&self) -> bool {
        self.client.as_ref().is_some_and(|c| c.pose().is_some())
    }

    /// Where to draw the vehicle now, in Godot's frame (+Y up, forward is -Z), smoothed between state messages.
    /// The identity transform before the first state arrives.
    #[func]
    fn get_pose(&self) -> Transform3D {
        let Some(pose) = self.client.as_ref().and_then(|c| c.pose()) else { return Transform3D::IDENTITY };
        let rotation = Quaternion::new(pose.att.x as f32, pose.att.y as f32, pose.att.z as f32, pose.att.w as f32);
        let origin = Vector3::new(pose.pos.x as f32, pose.pos.y as f32, pose.pos.z as f32);
        Transform3D::new(Basis::from_quaternion(rotation), origin)
    }

    /// The newest telemetry, or an empty Dictionary before the first state: `time_s`, `altitude_m`, `speed_mps`,
    /// `climb_mps`, `battery_voltage_v`, `battery_current_a`, `motor_cmd` (PackedFloat32Array), `motors_spinning`,
    /// `tx_enabled`, `link_up`, `lq_pct`, `rssi_dbm`, `running`, `overruns`, `fc_restarts` and `age_s`.
    #[func]
    fn get_telemetry(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(t) = self.client.as_ref().and_then(|c| c.telemetry()) else { return d };
        let motors: Vec<f32> = t.motor_cmd.iter().map(|m| *m as f32).collect();
        d.set("time_s", t.time_s);
        d.set("altitude_m", t.altitude_m);
        d.set("speed_mps", t.speed_mps);
        d.set("climb_mps", t.climb_mps);
        d.set("battery_voltage_v", t.battery_voltage_v);
        d.set("battery_current_a", t.battery_current_a);
        d.set("motor_cmd", &PackedFloat32Array::from(motors.as_slice()));
        d.set("motors_spinning", t.motors_spinning);
        d.set("tx_enabled", t.tx_enabled);
        d.set("link_up", t.link_up);
        d.set("lq_pct", t.lq_pct);
        d.set("rssi_dbm", t.rssi_dbm);
        d.set("running", t.running);
        d.set("overruns", t.overruns as i64);
        d.set("fc_restarts", i64::from(t.fc_restarts));
        d.set("age_s", t.age_s);
        d
    }
}

impl OfsClient {
    fn send(&self, command: Command) {
        if let Some(client) = &self.client {
            client.send(command);
        }
    }
}

fn string_key(d: &VarDictionary, key: &str) -> Option<String> {
    d.get(key).and_then(|v| v.try_to::<GString>().ok()).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

fn number_key(d: &VarDictionary, key: &str) -> Option<i64> {
    d.get(key).and_then(|v| v.try_to::<i64>().ok())
}

fn parse_settings(d: &VarDictionary) -> Result<Settings, String> {
    let quad_path = string_key(d, "quad_path").ok_or("`quad_path` is required")?;
    let mut settings = Settings::new(quad_path);
    if let Some(addr) = string_key(d, "server_addr") {
        settings.server_addr = addr;
    }
    if let Some(program) = string_key(d, "server_bin") {
        let mut env = Vec::new();
        if let Some(vars) = d.get("env").and_then(|v| v.try_to::<VarDictionary>().ok()) {
            for (key, value) in vars.iter_shared() {
                let (Ok(key), Ok(value)) = (key.try_to::<GString>(), value.try_to::<GString>()) else {
                    return Err("`env` must map strings to strings".into());
                };
                env.push((key.to_string(), value.to_string()));
            }
        }
        settings.launch = Some(LaunchSpec {
            program: PathBuf::from(program),
            data_dir: PathBuf::from(string_key(d, "data_dir").unwrap_or_else(|| ".ofs-data".into())),
            env,
            log_file: string_key(d, "log_file").map(PathBuf::from),
        });
    }
    if let Some(seed) = number_key(d, "seed") {
        settings.seed = u64::try_from(seed).map_err(|_| "`seed` must not be negative")?;
    }
    if let Some(open_loop) = d.get("open_loop").and_then(|v| v.try_to::<bool>().ok()) {
        settings.open_loop_fc = open_loop;
    }
    match string_key(d, "overrun_policy").as_deref() {
        None | Some("warn") => settings.overrun_policy = OverrunPolicy::Warn,
        Some("slow") => settings.overrun_policy = OverrunPolicy::Slow,
        Some(other) => return Err(format!("`overrun_policy` must be \"warn\" or \"slow\", not \"{other}\"")),
    }
    if let Some(hz) = number_key(d, "state_rate_hz") {
        settings.state_rate_hz = u32::try_from(hz).map_err(|_| "`state_rate_hz` is out of range")?;
    }
    if let Some(hz) = number_key(d, "stick_rate_hz") {
        settings.stick_rate_hz = u32::try_from(hz).map_err(|_| "`stick_rate_hz` is out of range")?;
    }
    if let Some(seconds) = number_key(d, "launch_timeout_s") {
        settings.launch_timeout = Duration::from_secs(u64::try_from(seconds).map_err(|_| "`launch_timeout_s` is out of range")?);
    }
    Ok(settings)
}
```

Run: `cargo build -p ofs-godot`.

Expected: compiles clean with no warnings; produces `target/debug/ofs_godot.dll` (Windows) or `target/debug/libofs_godot.so` (Linux), about 7 MB.

- [ ] **Step 3: Create the project skeleton.** `godot/ofs.gdextension` — exactly (the library paths point at the workspace's `target/`; do not redirect `CARGO_TARGET_DIR` when running Godot):

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.6
reloadable = true

[libraries]
windows.debug.x86_64 = "res://../target/debug/ofs_godot.dll"
windows.release.x86_64 = "res://../target/release/ofs_godot.dll"
linux.debug.x86_64 = "res://../target/debug/libofs_godot.so"
linux.release.x86_64 = "res://../target/release/libofs_godot.so"
```

`godot/project.godot` — exactly:

```
; Engine configuration file.
config_version=5

[application]

config/name="Open FPV Sim"
run/main_scene="res://main.tscn"
config/features=PackedStringArray("4.7", "GL Compatibility")

[display]

window/size/viewport_width=1280
window/size/viewport_height=720
window/stretch/mode="canvas_items"
window/stretch/aspect="expand"

[rendering]

renderer/rendering_method="gl_compatibility"
renderer/rendering_method.mobile="gl_compatibility"
anti_aliasing/quality/msaa_3d=2
```

- [ ] **Step 4: Verify the extension loads.** One import so Godot indexes the project, then a throwaway smoke script (not committed):

```bash
GODOT=path/to/Godot_v4.7.2-stable_win64_console.exe   # or the Linux binary
timeout -k 5 120 "$GODOT" --headless --path godot --import
cat > godot/tests/smoke_extension.gd <<'EOF'
extends SceneTree
func _initialize() -> void:
	var ok := ClassDB.class_exists("OfsClient")
	print("OfsClient registered: ", ok)
	quit(0 if ok else 1)
EOF
timeout -k 5 60 "$GODOT" --headless --path godot -s res://tests/smoke_extension.gd; echo "rc=$?"
rm -f godot/tests/smoke_extension.gd godot/tests/smoke_extension.gd.uid
```

Expected:
- the import exits 0;
- the smoke run prints `Initialize godot-rust (API v4.6.stable.official, runtime v4.7.2.stable.official, safeguards strict)` and `OfsClient registered: true`, and exits 0.

Remember: `timeout -k` on every Godot invocation — a script with a runtime error never exits on its own (Decision 10).

- [ ] **Step 5: Commit** (Godot has generated `*.uid` files for the new scripts — commit them):

```bash
git add .gitattributes .gitignore Cargo.lock crates/ofs-godot godot
git commit -m "feat(godot): the OfsClient GDExtension and the Godot project skeleton"
```

---

### Task 6: Input mapping and settings, with the headless test harness

**Files:**
- Create: `godot/scripts/controls.gd`, `godot/scripts/settings.gd`, `godot/tests/testing.gd`, `godot/tests/run_tests.gd`, `godot/tests/test_controls.gd`, `godot/tests/test_settings.gd`

**Interfaces:**
- Produces:
  - `Controls`: turns joystick axes, buttons and keys into the transmitter's sticks; per-channel bindings (roll, pitch, yaw, throttle, aux1–aux4) with invert/curve; saved to and loaded from `user://ofs_client_controls.json`;
  - `AppSettings`: defaults < `[client]` in `user://ofs_client.cfg` < `OFS_*` environment < CLI flags after `--` (server address, server binary, data dir, sticks rate);
  - the harness: `testing.gd` (an `ok`/`eq`/`near` assertion helper) and `run_tests.gd` (the suite runner; exit 1 on any failed check).

- [ ] **Step 1: Write the harness and the controls mapper with its tests.** Three files, exactly:

`godot/tests/testing.gd`:

```gdscript
extends RefCounted
## A tiny assertion helper for the headless tests (`godot --headless -s res://tests/run_tests.gd`).
## A test file extends this and defines `test_*` methods (they may `await`) that call `ok`, `eq`, `near`.

var failures := 0
var checks := 0
var current := ""
## The SceneTree the suite runs in, for tests that need nodes in a tree.
var tree: SceneTree = null


func ok(condition: bool, message: String) -> void:
	checks += 1
	if not condition:
		failures += 1
		printerr("  FAIL [%s] %s" % [current, message])


func eq(actual, expected, message: String) -> void:
	ok(typeof(actual) == typeof(expected) and actual == expected, "%s: expected %s, got %s" % [message, str(expected), str(actual)])


func near(actual: float, expected: float, message: String, eps := 1e-6) -> void:
	ok(absf(actual - expected) <= eps, "%s: expected %s, got %s" % [message, str(expected), str(actual)])


## Adds a node to the tree and waits for its `_ready` (which runs on the next frame).
func add_to_tree(node: Node) -> void:
	tree.root.add_child(node)
	await tree.process_frame


## Runs every method whose name starts with `test_` and returns the number of failed checks.
func run_all() -> int:
	for method in get_method_list():
		var name: String = method["name"]
		if name.begins_with("test_"):
			current = name
			await call(name)
	return failures
```

`godot/scripts/controls.gd`:

```gdscript
extends RefCounted
## Turns joystick axes, buttons and keys into the transmitter's sticks.
##
## Every channel (roll, pitch, yaw, throttle, aux1..aux4) has one binding. A binding reads a value in [-1, 1]:
## roll/pitch/yaw/aux are used as they are, throttle becomes (value + 1) / 2. Sign conventions, as Betaflight sees
## them: roll +1 is right, pitch +1 is stick forward (nose down), yaw +1 is right, throttle 1 is full, aux +1 is the
## switch on (aux1 arms, aux2 selects Angle mode in the reference quad).
##
## `read` takes the input source as a parameter (the `Input` singleton in the game, a fake in the tests).

const CHANNELS := ["roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"]
const STICK_CHANNELS := ["roll", "pitch", "yaw"]
const PRESET_NAMES := ["radio", "gamepad", "keyboard"]

## device -1 means "the first connected controller".
var bindings: Dictionary = {}
var deadzone := 0.03
## Keyboard throttle: how far the throttle stick moves per second (throttle goes 0..1).
var throttle_ramp_per_s := 0.6
var preset_name := ""

var _ramp: Dictionary = {}
var _toggle: Dictionary = {}
var _was_down: Dictionary = {}


func _init(preset: String = "radio") -> void:
	apply_preset(preset)


## The bindings of a preset: "radio" (a USB radio in joystick mode, channels in AETR order), "gamepad" (Mode 2,
## Xbox layout: left stick throttle and yaw, right stick pitch and roll) or "keyboard" (for trying things out).
static func preset(name: String) -> Dictionary:
	match name:
		"gamepad":
			return {
				"deadzone": 0.08,
				"bindings": {
					"roll": _axis(2, false), "pitch": _axis(3, true), "yaw": _axis(0, false), "throttle": _axis(1, true),
					"aux1": _button(JOY_BUTTON_A), "aux2": _button(JOY_BUTTON_B),
					"aux3": _button(JOY_BUTTON_X), "aux4": _button(JOY_BUTTON_Y),
				},
			}
		"keyboard":
			return {
				"deadzone": 0.0,
				"bindings": {
					"roll": _pair(KEY_LEFT, KEY_RIGHT, 0.6), "pitch": _pair(KEY_DOWN, KEY_UP, 0.6),
					"yaw": _pair(KEY_Q, KEY_E, 0.8), "throttle": {"kind": "key_ramp", "down": KEY_S, "up": KEY_W},
					"aux1": {"kind": "key_toggle", "key": KEY_SPACE}, "aux2": {"kind": "key_toggle", "key": KEY_F},
					"aux3": {"kind": "none"}, "aux4": {"kind": "none"},
				},
			}
		_:
			return {
				"deadzone": 0.02,
				"bindings": {
					"roll": _axis(0, false), "pitch": _axis(1, false), "throttle": _axis(2, false), "yaw": _axis(3, false),
					"aux1": _axis(4, false), "aux2": _axis(5, false), "aux3": _axis(6, false), "aux4": _axis(7, false),
				},
			}


static func _axis(index: int, invert: bool) -> Dictionary:
	return {"kind": "axis", "device": -1, "index": index, "invert": invert}


static func _button(index: int) -> Dictionary:
	return {"kind": "button", "device": -1, "index": index}


static func _pair(neg: int, pos: int, scale: float) -> Dictionary:
	return {"kind": "key_pair", "neg": neg, "pos": pos, "scale": scale}


func apply_preset(name: String) -> void:
	var p := preset(name)
	preset_name = name if name in PRESET_NAMES else "radio"
	bindings = p["bindings"].duplicate(true)
	deadzone = p["deadzone"]
	reset_state()


func reset_state() -> void:
	_ramp.clear()
	_toggle.clear()
	_was_down.clear()


func set_binding(channel: String, binding: Dictionary) -> void:
	if channel in CHANNELS:
		bindings[channel] = binding
		preset_name = "custom"
		reset_state()


## "Axis 2 of any controller", "Button 0", "Key Space", ... for the controls screen.
func describe(channel: String) -> String:
	var b: Dictionary = bindings.get(channel, {"kind": "none"})
	var device := "any controller" if int(b.get("device", -1)) < 0 else "controller %d" % int(b.get("device", -1))
	match b.get("kind", "none"):
		"axis":
			return "axis %d of %s%s" % [int(b["index"]), device, " (inverted)" if b.get("invert", false) else ""]
		"button":
			return "button %d of %s (toggle)" % [int(b["index"]), device]
		"key_toggle":
			return "key %s (toggle)" % OS.get_keycode_string(int(b["key"]))
		"key_pair":
			return "keys %s / %s" % [OS.get_keycode_string(int(b["neg"])), OS.get_keycode_string(int(b["pos"]))]
		"key_ramp":
			return "keys %s / %s (hold)" % [OS.get_keycode_string(int(b["down"])), OS.get_keycode_string(int(b["up"]))]
	return "not bound"


func to_json() -> String:
	return JSON.stringify({"version": 1, "preset": preset_name, "deadzone": deadzone, "bindings": bindings}, "\t")


## Loads bindings saved by `to_json`. Returns false (changing nothing) when the text is not a valid controls file.
func from_json(text: String) -> bool:
	var json := JSON.new()
	if json.parse(text) != OK:
		return false
	var data = json.data
	if typeof(data) != TYPE_DICTIONARY or typeof(data.get("bindings")) != TYPE_DICTIONARY:
		return false
	var loaded := {}
	for channel in CHANNELS:
		var b = data["bindings"].get(channel, {"kind": "none"})
		if typeof(b) != TYPE_DICTIONARY:
			return false
		var clean := _clean_binding(b)
		if clean.is_empty():
			return false
		loaded[channel] = clean
	bindings = loaded
	deadzone = clampf(float(data.get("deadzone", 0.03)), 0.0, 0.5)
	preset_name = str(data.get("preset", "custom"))
	reset_state()
	return true


static func _clean_binding(b: Dictionary) -> Dictionary:
	# JSON turns integers into floats; bring the integer fields back and reject unknown kinds.
	var kind := str(b.get("kind", "none"))
	var out := {"kind": kind}
	for field in ["device", "index", "key", "neg", "pos", "down", "up"]:
		if b.has(field):
			out[field] = int(b[field])
	if b.has("invert"):
		out["invert"] = bool(b["invert"])
	if b.has("scale"):
		out["scale"] = float(b["scale"])
	var needs := {"none": [], "axis": ["index"], "button": ["index"], "key_toggle": ["key"], "key_pair": ["neg", "pos"], "key_ramp": ["down", "up"]}
	if not needs.has(kind):
		return {}
	for field in needs[kind]:
		if not out.has(field):
			return {}
	return out


## Reads the sticks. `focused` false (the game window lost focus) centres the sticks and closes the throttle;
## the aux switches stay as they are. `delta` is the time since the last read, for the keyboard throttle.
## Returns {roll, pitch, yaw, throttle, aux: [4 values], status}; `status` is a warning for the HUD or "".
func read(input, focused := true, delta := 0.0) -> Dictionary:
	var missing := false
	var values := {}
	for channel in CHANNELS:
		var v := _channel_value(channel, input, delta)
		if is_nan(v):
			missing = true
			v = -1.0 if channel == "throttle" or channel.begins_with("aux") else 0.0
		values[channel] = clampf(v, -1.0, 1.0) if is_finite(v) else 0.0
	var out := {
		"roll": _dead(values["roll"]), "pitch": _dead(values["pitch"]), "yaw": _dead(values["yaw"]),
		"throttle": (values["throttle"] + 1.0) / 2.0,
		"aux": [values["aux1"], values["aux2"], values["aux3"], values["aux4"]],
		"status": "",
	}
	if missing:
		out["status"] = "Controller not connected: the sticks are centred"
	if not focused:
		out["roll"] = 0.0
		out["pitch"] = 0.0
		out["yaw"] = 0.0
		out["throttle"] = 0.0
		_ramp.erase("throttle")
	return out


func _dead(v: float) -> float:
	if absf(v) <= deadzone:
		return 0.0
	return signf(v) * (absf(v) - deadzone) / (1.0 - deadzone)


## The channel's value in [-1, 1], or NAN when its controller is not there.
func _channel_value(channel: String, input, delta: float) -> float:
	var b: Dictionary = bindings.get(channel, {"kind": "none"})
	var neutral := -1.0 if channel == "throttle" or channel.begins_with("aux") else 0.0
	match b.get("kind", "none"):
		"axis":
			var device := _device(b, input)
			if device < 0:
				return NAN
			var v: float = input.get_joy_axis(device, int(b["index"]))
			if not is_finite(v):
				return NAN
			return -v if b.get("invert", false) else v
		"button":
			var device := _device(b, input)
			if device < 0:
				return NAN
			if _pressed_edge("%s/button" % channel, input.is_joy_button_pressed(device, int(b["index"]))):
				_toggle[channel] = not _toggle.get(channel, false)
			return 1.0 if _toggle.get(channel, false) else -1.0
		"key_toggle":
			if _pressed_edge("%s/key" % channel, input.is_physical_key_pressed(int(b["key"]))):
				_toggle[channel] = not _toggle.get(channel, false)
			return 1.0 if _toggle.get(channel, false) else -1.0
		"key_pair":
			var scale := float(b.get("scale", 1.0))
			var v := 0.0
			if input.is_physical_key_pressed(int(b["pos"])):
				v += scale
			if input.is_physical_key_pressed(int(b["neg"])):
				v -= scale
			return v
		"key_ramp":
			var speed := throttle_ramp_per_s * 2.0 * delta
			var v: float = _ramp.get(channel, -1.0)
			if input.is_physical_key_pressed(int(b["up"])):
				v += speed
			if input.is_physical_key_pressed(int(b["down"])):
				v -= speed
			v = clampf(v, -1.0, 1.0)
			_ramp[channel] = v
			return v
	return neutral


## The controller a binding reads: its own device if connected, else (device -1) the first connected one; -1 if none.
func _device(b: Dictionary, input) -> int:
	var pads: Array = input.get_connected_joypads()
	var wanted := int(b.get("device", -1))
	if wanted >= 0:
		return wanted if wanted in pads else -1
	return int(pads[0]) if pads.size() > 0 else -1


func _pressed_edge(id: String, down: bool) -> bool:
	var edge: bool = down and not _was_down.get(id, false)
	_was_down[id] = down
	return edge
```

`godot/tests/test_controls.gd`:

```gdscript
extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")


## Stands in for the `Input` singleton.
class FakeInput:
	extends RefCounted
	var pads: Array = [0]
	var axes: Dictionary = {}  # "device/axis" -> value
	var buttons: Dictionary = {}  # "device/button" -> bool
	var keys: Dictionary = {}  # keycode -> bool

	func get_connected_joypads() -> Array:
		return pads

	func get_joy_axis(device: int, axis: int) -> float:
		return axes.get("%d/%d" % [device, axis], 0.0)

	func is_joy_button_pressed(device: int, button: int) -> bool:
		return buttons.get("%d/%d" % [device, button], false)

	func is_physical_key_pressed(key: int) -> bool:
		return keys.get(key, false)


func test_radio_preset_reads_aetr_axes_straight() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.axes = {"0/0": 0.5, "0/1": -0.25, "0/2": 1.0, "0/3": 0.75, "0/4": 1.0, "0/5": -1.0}
	var s := c.read(input)
	near(s["roll"], (0.5 - 0.02) / 0.98, "roll")
	near(s["pitch"], -(0.25 - 0.02) / 0.98, "pitch")
	near(s["yaw"], (0.75 - 0.02) / 0.98, "yaw")
	near(s["throttle"], 1.0, "throttle at the top")
	eq(s["aux"], [1.0, -1.0, 0.0, 0.0], "aux1 on, aux2 off, the two idle axes read as the middle")
	eq(s["status"], "", "no warning")


func test_throttle_maps_the_whole_axis_to_zero_to_one() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	for pair in [[-1.0, 0.0], [0.0, 0.5], [1.0, 1.0]]:
		input.axes = {"0/2": pair[0]}
		near(c.read(input)["throttle"], pair[1], "throttle axis %s" % str(pair[0]))


func test_gamepad_preset_pushes_up_for_throttle_and_forward_for_pitch() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	input.axes = {"0/1": -1.0, "0/3": -1.0, "0/0": 0.0, "0/2": 1.0}  # left stick up, right stick forward and right
	var s := c.read(input)
	near(s["throttle"], 1.0, "left stick up is full throttle")
	near(s["pitch"], 1.0, "right stick forward is positive pitch")
	near(s["roll"], 1.0, "right stick right is positive roll")
	near(s["yaw"], 0.0, "yaw at rest")


func test_deadzone_rescales_what_is_left() -> void:
	var c := Controls.new("radio")
	c.deadzone = 0.1
	var input := FakeInput.new()
	input.axes = {"0/0": 0.05}
	near(c.read(input)["roll"], 0.0, "inside the deadzone")
	input.axes = {"0/0": 0.55}
	near(c.read(input)["roll"], 0.5, "(0.55 - 0.1) / 0.9")
	input.axes = {"0/0": 1.0}
	near(c.read(input)["roll"], 1.0, "full deflection stays full")
	input.axes = {"0/0": -1.0}
	near(c.read(input)["roll"], -1.0, "and negative")


func test_buttons_toggle_on_the_press_not_while_held() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	eq(c.read(input)["aux"][0], -1.0, "arm starts off")
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	eq(c.read(input)["aux"][0], 1.0, "pressed: armed")
	eq(c.read(input)["aux"][0], 1.0, "held: still armed, no re-toggle")
	input.buttons = {}
	eq(c.read(input)["aux"][0], 1.0, "released: stays armed")
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	eq(c.read(input)["aux"][0], -1.0, "pressed again: disarmed")


func test_keyboard_preset() -> void:
	var c := Controls.new("keyboard")
	var input := FakeInput.new()
	input.pads = []
	input.keys = {KEY_RIGHT: true, KEY_UP: true, KEY_E: true, KEY_W: true}
	var s := c.read(input, true, 0.5)
	near(s["roll"], 0.6, "right arrow")
	near(s["pitch"], 0.6, "up arrow is forward")
	near(s["yaw"], 0.8, "E yaws right")
	near(s["throttle"], 0.3, "W held for half a second at 0.6 per second")
	eq(s["status"], "", "the keyboard needs no controller")
	s = c.read(input, true, 10.0)
	near(s["throttle"], 1.0, "the throttle stops at the top")
	input.keys = {KEY_S: true}
	s = c.read(input, true, 10.0)
	near(s["throttle"], 0.0, "and at the bottom")
	input.keys = {KEY_SPACE: true}
	eq(c.read(input)["aux"][0], 1.0, "space arms")
	input.keys = {KEY_LEFT: true, KEY_DOWN: true, KEY_Q: true}
	s = c.read(input)
	near(s["roll"], -0.6, "left arrow")
	near(s["pitch"], -0.6, "down arrow")
	near(s["yaw"], -0.8, "Q yaws left")


func test_a_missing_controller_centres_the_sticks_and_warns() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.pads = []
	input.axes = {"0/0": 1.0, "0/2": 1.0}
	var s := c.read(input)
	eq([s["roll"], s["pitch"], s["yaw"], s["throttle"]], [0.0, 0.0, 0.0, 0.0], "centred sticks and closed throttle")
	eq(s["aux"], [-1.0, -1.0, -1.0, -1.0], "switches off")
	ok(s["status"].contains("not connected"), "a warning for the HUD: %s" % s["status"])
	# A binding to a specific device that has been unplugged.
	c.set_binding("roll", {"kind": "axis", "device": 3, "index": 0, "invert": false})
	input.pads = [0]
	input.axes = {"0/0": 1.0, "3/0": 1.0}
	s = c.read(input)
	near(s["roll"], 0.0, "device 3 is not connected")
	ok(s["status"] != "", "and the HUD is told")
	input.pads = [0, 3]
	s = c.read(input)
	near(s["roll"], 1.0, "plugged back in")
	eq(s["status"], "", "the warning clears")


func test_losing_window_focus_centres_the_sticks_but_keeps_the_switches() -> void:
	var c := Controls.new("gamepad")
	var input := FakeInput.new()
	input.axes = {"0/1": -1.0, "0/2": 1.0}
	input.buttons = {"0/%d" % JOY_BUTTON_A: true}
	var s := c.read(input, true)
	near(s["throttle"], 1.0, "focused")
	eq(s["aux"][0], 1.0, "armed")
	s = c.read(input, false)
	eq([s["roll"], s["pitch"], s["yaw"], s["throttle"]], [0.0, 0.0, 0.0, 0.0], "unfocused: centred")
	eq(s["aux"][0], 1.0, "the arm switch stays on")


func test_unfocused_keyboard_throttle_starts_again_from_zero() -> void:
	var c := Controls.new("keyboard")
	var input := FakeInput.new()
	input.pads = []
	input.keys = {KEY_W: true}
	near(c.read(input, true, 1.0)["throttle"], 0.6, "ramped up")
	input.keys = {}
	near(c.read(input, false, 0.1)["throttle"], 0.0, "focus lost")
	near(c.read(input, true, 0.1)["throttle"], 0.0, "and it stays closed when focus returns")


func test_non_finite_axis_values_are_treated_as_a_missing_controller() -> void:
	var c := Controls.new("radio")
	var input := FakeInput.new()
	input.axes = {"0/0": NAN, "0/1": INF}
	var s := c.read(input)
	for key in ["roll", "pitch", "yaw", "throttle"]:
		ok(is_finite(s[key]), "%s is finite" % key)
	ok(s["status"] != "", "reported")


func test_bindings_survive_a_json_round_trip() -> void:
	for name in Controls.PRESET_NAMES:
		var a := Controls.new(name)
		a.deadzone = 0.07
		var b := Controls.new("radio")
		ok(b.from_json(a.to_json()), "%s loads" % name)
		eq(b.bindings, a.bindings, "%s bindings" % name)
		near(b.deadzone, 0.07, "%s deadzone" % name)
		eq(b.preset_name, name, "%s preset name" % name)


func test_invalid_controls_files_are_rejected_and_change_nothing() -> void:
	var c := Controls.new("keyboard")
	var before := c.bindings.duplicate(true)
	for text in ["", "not json", "[]", "{\"bindings\": 3}", "{\"bindings\": {\"roll\": {\"kind\": \"telepathy\"}}}",
			"{\"bindings\": {\"roll\": {\"kind\": \"axis\"}}}", "{\"bindings\": {\"roll\": 5}}"]:
		ok(not c.from_json(text), "rejects %s" % text)
	eq(c.bindings, before, "bindings unchanged")


func test_describe_names_each_kind_of_binding() -> void:
	var c := Controls.new("gamepad")
	eq(c.describe("throttle"), "axis 1 of any controller (inverted)", "axis")
	eq(c.describe("aux1"), "button 0 of any controller (toggle)", "button")
	c.apply_preset("keyboard")
	eq(c.describe("aux1"), "key Space (toggle)", "toggle key")
	eq(c.describe("aux3"), "not bound", "unbound")
```

`godot/tests/run_tests.gd` — for this task only the first two suites (Task 8 replaces this file with the final four):

```gdscript
extends SceneTree
## Runs the unit tests: godot --headless --path godot -s res://tests/run_tests.gd
## Exit code 0 when every check passed. (Run `godot --headless --path godot --import` once first.)

const SUITES := [
	"res://tests/test_controls.gd",
	"res://tests/test_settings.gd",
]


func _initialize() -> void:
	_run()


func _run() -> void:
	var failures := 0
	var checks := 0
	for path in SUITES:
		var suite = load(path).new()
		suite.tree = self
		var failed: int = await suite.run_all()
		print("%s: %d checks, %d failed" % [path, suite.checks, failed])
		failures += failed
		checks += suite.checks
	print("TOTAL: %d checks, %d failed" % [checks, failures])
	quit(1 if failures > 0 else 0)
```

- [ ] **Step 2: Write the settings module and its tests.** Two files, exactly:

`godot/scripts/settings.gd`:

```gdscript
extends RefCounted
## The pilot client's settings. Later sources win: built-in defaults, then the `[client]` section of
## user://ofs_client.cfg, then OFS_* environment variables, then command-line flags after `--`
## (`godot --path godot -- --open-loop --quad=quads/other.toml`).

const CONFIG_PATH := "user://ofs_client.cfg"

## Environment variable -> setting.
const ENV_NAMES := {
	"OFS_SIM_BIN": "server_bin",
	"OFS_SERVER_ADDR": "server_addr",
	"OFS_QUAD": "quad_path",
	"OFS_DATA_DIR": "data_dir",
	"OFS_OPEN_LOOP": "open_loop",
	"OFS_SITL_LAUNCH": "sitl_launch",
}
## Command-line flag -> setting (`--name=value`; `--open-loop` alone means true).
const FLAG_NAMES := {
	"--sim": "server_bin",
	"--server": "server_addr",
	"--quad": "quad_path",
	"--data-dir": "data_dir",
	"--open-loop": "open_loop",
	"--sitl-launch": "sitl_launch",
}


## The repository root when the project runs from a checkout (godot/ sits one level below it).
static func repo_root() -> String:
	return ProjectSettings.globalize_path("res://").path_join("..").simplify_path()


static func defaults() -> Dictionary:
	var root := repo_root()
	var exe := "ofs-sim.exe" if OS.get_name() == "Windows" else "ofs-sim"
	var server_bin := exe  # found on PATH
	for profile in ["release", "debug"]:
		var candidate := root.path_join("target").path_join(profile).path_join(exe)
		if FileAccess.file_exists(candidate):
			server_bin = candidate
			break
	return {
		"server_addr": "127.0.0.1:50051",
		"server_bin": server_bin,
		"data_dir": root.path_join(".ofs-data"),
		"quad_path": root.path_join("quads").path_join("opendrone-5f-freestyle.toml"),
		"sitl_launch": "",
		"open_loop": false,
		"seed": 1,
		"overrun_policy": "warn",
		"state_rate_hz": 240,
		"stick_rate_hz": 250,
	}


## Merges the sources. `config` holds the `[client]` values, `env` the OFS_* variables that are set, `args` the
## command-line flags after `--`.
static func resolve(config: Dictionary, env: Dictionary, args: PackedStringArray) -> Dictionary:
	var s := defaults()
	for key in config:
		if s.has(key):
			s[key] = _coerce(s[key], config[key])
	for name in env:
		if ENV_NAMES.has(name):
			var key: String = ENV_NAMES[name]
			s[key] = _coerce(s[key], env[name])
	for arg in args:
		var parts := arg.split("=", true, 1)
		if FLAG_NAMES.has(parts[0]):
			var key: String = FLAG_NAMES[parts[0]]
			s[key] = _coerce(s[key], parts[1] if parts.size() > 1 else "true")
	return s


## Brings a value from text or a config file to the type of the default it replaces.
static func _coerce(default, value):
	match typeof(default):
		TYPE_BOOL:
			if typeof(value) == TYPE_BOOL:
				return value
			return str(value).to_lower() in ["1", "true", "yes", "on"]
		TYPE_INT:
			return int(value) if str(value).is_valid_int() else default
		TYPE_STRING:
			return str(value)
	return value


static func load_settings() -> Dictionary:
	var config := {}
	var file := ConfigFile.new()
	if file.load(CONFIG_PATH) == OK:
		for key in file.get_section_keys("client") if file.has_section("client") else PackedStringArray():
			config[key] = file.get_value("client", key)
	var env := {}
	for name in ENV_NAMES:
		if OS.has_environment(name):
			env[name] = OS.get_environment(name)
	return resolve(config, env, OS.get_cmdline_user_args())


## The dictionary `OfsClient.start` takes.
static func to_client_dict(s: Dictionary) -> Dictionary:
	var d := {
		"quad_path": s["quad_path"],
		"server_addr": s["server_addr"],
		"seed": s["seed"],
		"open_loop": s["open_loop"],
		"overrun_policy": s["overrun_policy"],
		"state_rate_hz": s["state_rate_hz"],
		"stick_rate_hz": s["stick_rate_hz"],
	}
	if s["server_bin"] != "":
		d["server_bin"] = s["server_bin"]
		d["data_dir"] = s["data_dir"]
		d["log_file"] = String(s["data_dir"]).path_join("ofs-sim.log")
		if s["sitl_launch"] != "":
			d["env"] = {"OFS_SITL_LAUNCH": s["sitl_launch"]}
	return d
```

`godot/tests/test_settings.gd`:

```gdscript
extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")


func test_defaults_point_into_the_checkout() -> void:
	var s := AppSettings.defaults()
	var root := AppSettings.repo_root()
	ok(not root.contains(".."), "the repository root is simplified: %s" % root)
	ok(s["quad_path"].begins_with(root) and s["quad_path"].ends_with("opendrone-5f-freestyle.toml"), "quad path %s" % s["quad_path"])
	ok(FileAccess.file_exists(s["quad_path"]), "the reference quad exists at %s" % s["quad_path"])
	eq(s["server_addr"], "127.0.0.1:50051", "server address")
	eq(s["open_loop"], false, "Betaflight by default")
	ok(s["server_bin"].ends_with("ofs-sim") or s["server_bin"].ends_with("ofs-sim.exe"), "server binary %s" % s["server_bin"])


func test_later_sources_win() -> void:
	var s := AppSettings.resolve(
		{"server_addr": "10.0.0.1:1", "seed": 7, "state_rate_hz": 120},
		{"OFS_SERVER_ADDR": "127.0.0.1:6000", "OFS_OPEN_LOOP": "1"},
		PackedStringArray(["--server=127.0.0.1:7000", "--quad=quads/other.toml"]))
	eq(s["server_addr"], "127.0.0.1:7000", "the command line beats the environment, which beats the file")
	eq(s["open_loop"], true, "the environment turns open loop on")
	eq(s["seed"], 7, "the file's seed stays")
	eq(s["state_rate_hz"], 120, "and its rate")
	eq(s["quad_path"], "quads/other.toml", "the quad from the command line")


func test_flags_without_a_value_mean_true_and_junk_is_ignored() -> void:
	var s := AppSettings.resolve({}, {}, PackedStringArray(["--open-loop", "--unknown=1", "positional"]))
	eq(s["open_loop"], true, "--open-loop")
	var t := AppSettings.resolve({}, {"OFS_OPEN_LOOP": "no", "NOT_OURS": "x"}, PackedStringArray())
	eq(t["open_loop"], false, "OFS_OPEN_LOOP=no")
	var u := AppSettings.resolve({"seed": "not a number", "bogus": 1}, {}, PackedStringArray())
	eq(u["seed"], 1, "a seed that is not a number keeps the default")
	ok(not u.has("bogus"), "unknown keys are dropped")


func test_the_client_dictionary_launches_a_server_only_when_one_is_configured() -> void:
	var s := AppSettings.defaults()
	s["server_bin"] = "C:/x/ofs-sim.exe"
	s["data_dir"] = "C:/x/data"
	s["sitl_launch"] = "wsl.exe -d Ubuntu -e /home/me/betaflight_SITL.elf"
	var d := AppSettings.to_client_dict(s)
	eq(d["server_bin"], "C:/x/ofs-sim.exe", "server binary")
	eq(d["log_file"], "C:/x/data/ofs-sim.log", "log file")
	eq(d["env"], {"OFS_SITL_LAUNCH": "wsl.exe -d Ubuntu -e /home/me/betaflight_SITL.elf"}, "SITL launch for the server")
	s["server_bin"] = ""
	d = AppSettings.to_client_dict(s)
	ok(not d.has("server_bin") and not d.has("env"), "attach only")
	eq(d["quad_path"], s["quad_path"], "quad path")
```

- [ ] **Step 3: Run the suites.**

```bash
timeout -k 5 120 "$GODOT" --headless --path godot --import
timeout -k 5 120 "$GODOT" --headless --path godot -s res://tests/run_tests.gd
```

Expected:
- `res://tests/test_controls.gd: 76 checks, 0 failed`
- `res://tests/test_settings.gd: 20 checks, 0 failed`
- `TOTAL: 96 checks, 0 failed`, exit 0.

If Godot prints JSON parse noise, check that `controls.gd` uses the quiet `JSON.new().parse()` API rather than `JSON.parse_string`.

- [ ] **Step 4: Repeat three times** (expect three clean `TOTAL: 96 checks, 0 failed`).

- [ ] **Step 5: Commit** (including the `*.uid` files Godot generated):

```bash
git add godot
git commit -m "feat(godot): input mapping and settings, with the headless test harness"
```
---

### Task 7: The grey-box world, the drone, and the cameras

**Files:**
- Create: `godot/world/grid.gdshader`, `godot/world/world.gd`, `godot/drone/drone.gd`, `godot/drone/fpv_camera.gd`, `godot/drone/chase_camera.gd`, `godot/ui/lens.gdshader`, `godot/ui/lens.gd`

**Interfaces:**
- Consumes: `ofs_client::Pose` via `OfsClient` (Task 5).
- Produces:
  - `World`: sky, sun, a metre-grid ground (the simulator knows a plane at height 0; the world only draws), launch pad, pylons, gates, a few buildings;
  - `Drone`: a small quad drawn at the reported pose, props turning with the motor commands;
  - `FpvCamera`: wide horizontal FOV, tilted up so the horizon sits like a real FPV cam, with the lens shader (barrel distortion, exposure, vignette) as a full-screen layer;
  - `ChaseCamera`: a third-person camera behind the drone for finding out what the drone is doing when FPV feels wrong.

- [ ] **Step 1: Create the files.** Each exactly:

`godot/world/grid.gdshader`:

```glsl
shader_type spatial;
render_mode cull_back, diffuse_lambert, specular_disabled;
// The ground: flat grey-green with a 1 m grid and a stronger line every 10 m, drawn from world position so
// it needs no UVs. Lines fade out with distance (when a cell shrinks below a couple of pixels) to avoid moire.

uniform vec3 base_color : source_color = vec3(0.30, 0.34, 0.29);
uniform vec3 line_color : source_color = vec3(0.50, 0.55, 0.48);
uniform vec3 major_color : source_color = vec3(0.82, 0.84, 0.74);
uniform float cell = 1.0;
uniform float major_every = 10.0;

varying vec3 world_pos;

void vertex() {
	world_pos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

// 1 on a line, 0 between lines; the line is about `width` pixels wide and gone when cells get tiny.
float grid(vec2 p, float size, float width) {
	vec2 q = p / size;
	vec2 w = max(fwidth(q), vec2(1e-5));
	vec2 g = abs(fract(q - 0.5) - 0.5) / w;
	float line = 1.0 - clamp(min(g.x, g.y) - (width - 1.0), 0.0, 1.0);
	float fade = 1.0 - smoothstep(0.25, 0.5, max(w.x, w.y));
	return line * fade;
}

void fragment() {
	vec2 p = world_pos.xz;
	vec3 color = base_color;
	color = mix(color, line_color, grid(p, cell, 1.0) * 0.55);
	color = mix(color, major_color, grid(p, cell * major_every, 2.0) * 0.8);
	ALBEDO = color;
	ROUGHNESS = 1.0;
}
```

`godot/world/world.gd`:

```gdscript
extends Node3D
## A grey-box world: sky, sun, a ground plane with a metre grid, a launch pad, pylons, gates and a few buildings.
## It is only drawn. The simulator knows the ground (a plane at height 0) and nothing else, so nothing here
## collides with the drone.
##
## Godot's frame: +Y up, forward (north, where the drone starts facing) is -Z, +X is right (east).

const GRID_SHADER := preload("res://world/grid.gdshader")

const ORANGE := Color(1.0, 0.45, 0.1)
const WHITE := Color(0.92, 0.92, 0.9)
const GREEN := Color(0.2, 0.8, 0.35)
const YELLOW := Color(0.95, 0.8, 0.15)
const CONCRETE := Color(0.62, 0.64, 0.66)


func _ready() -> void:
	_add_environment()
	_add_ground()
	_add_launch_pad()
	_add_pylons()
	_add_gates()
	_add_buildings()


func _add_environment() -> void:
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.30, 0.48, 0.78)
	sky_material.sky_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_bottom_color = Color(0.40, 0.45, 0.40)
	var sky := Sky.new()
	sky.sky_material = sky_material
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.8
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	add_child(world_environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	sun.light_energy = 1.1
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 150.0
	add_child(sun)


func _add_ground() -> void:
	var plane := PlaneMesh.new()
	plane.size = Vector2(6000.0, 6000.0)
	var material := ShaderMaterial.new()
	material.shader = GRID_SHADER
	var ground := MeshInstance3D.new()
	ground.name = "Ground"
	ground.mesh = plane
	ground.material_override = material
	add_child(ground)


func _add_launch_pad() -> void:
	_box("LaunchPad", Vector3(0, 0.01, 0), Vector3(3.0, 0.02, 3.0), Color(0.78, 0.78, 0.76))


func _add_pylons() -> void:
	# Two rows of pylons along the flight direction, every 15 m.
	for i in range(1, 9):
		var z := -15.0 * i
		_cylinder("Pylon%dL" % i, Vector3(-6.0, 1.5, z), 0.15, 3.0, ORANGE if i % 2 == 0 else WHITE)
		_cylinder("Pylon%dR" % i, Vector3(6.0, 1.5, z), 0.15, 3.0, WHITE if i % 2 == 0 else ORANGE)


func _add_gates() -> void:
	var gates := [[-30.0, GREEN], [-60.0, YELLOW], [-90.0, GREEN]]
	for i in gates.size():
		var z: float = gates[i][0]
		var color: Color = gates[i][1]
		_box("Gate%dPostL" % i, Vector3(-1.6, 1.25, z), Vector3(0.12, 2.5, 0.12), color)
		_box("Gate%dPostR" % i, Vector3(1.6, 1.25, z), Vector3(0.12, 2.5, 0.12), color)
		_box("Gate%dBar" % i, Vector3(0.0, 2.5, z), Vector3(3.32, 0.12, 0.12), color)


func _add_buildings() -> void:
	_box("BuildingA", Vector3(-45.0, 7.0, -110.0), Vector3(14.0, 14.0, 12.0), CONCRETE)
	_box("BuildingB", Vector3(38.0, 11.0, -75.0), Vector3(10.0, 22.0, 10.0), CONCRETE.darkened(0.15))
	_box("BuildingC", Vector3(70.0, 5.0, -140.0), Vector3(30.0, 10.0, 18.0), CONCRETE.lightened(0.1))
	_box("BuildingD", Vector3(-80.0, 9.0, -30.0), Vector3(12.0, 18.0, 12.0), CONCRETE.darkened(0.05))


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.9
	return material


func _box(node_name: String, center: Vector3, size: Vector3, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = size
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)


func _cylinder(node_name: String, center: Vector3, radius: float, height: float, color: Color) -> void:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)
```

`godot/drone/drone.gd`:

```gdscript
extends Node3D
## The drone: a small quad drawn at the pose the simulator reports. Propellers turn with the motor commands.
##
## Godot's frame: forward is -Z, right is +X, up is +Y. The motor order is Betaflight's quad-X (M1 rear right,
## M2 front right, M3 rear left, M4 front left), as in quads/opendrone-5f-freestyle.toml.

const ARM := 0.08
const MOTORS := [Vector3(ARM, 0, ARM), Vector3(ARM, 0, -ARM), Vector3(-ARM, 0, ARM), Vector3(-ARM, 0, -ARM)]
## +1: the propeller turns clockwise seen from above (Betaflight's default direction for each motor).
const SPIN := [1.0, -1.0, -1.0, 1.0]
## A real propeller turns far too fast to show; this is the drawn speed at a full motor command, in rad/s.
const DRAWN_SPIN := 60.0

var _props: Array[Node3D] = []


func _ready() -> void:
	_add_box("Body", Vector3(0, 0, 0), Vector3(0.04, 0.03, 0.08), Color(0.12, 0.12, 0.14))
	_add_box("Battery", Vector3(0, 0.025, 0.01), Vector3(0.035, 0.025, 0.09), Color(0.9, 0.55, 0.1))
	for sign_x in [-1.0, 1.0]:
		var arm := _add_box("Arm", Vector3.ZERO, Vector3(0.24, 0.008, 0.014), Color(0.2, 0.2, 0.22))
		arm.rotation_degrees.y = 45.0 * sign_x
	for i in MOTORS.size():
		var pivot := Node3D.new()
		pivot.name = "Prop%d" % (i + 1)
		pivot.position = MOTORS[i] + Vector3(0, 0.012, 0)
		add_child(pivot)
		_props.append(pivot)
		var blade := MeshInstance3D.new()
		var blade_mesh := BoxMesh.new()
		blade_mesh.size = Vector3(0.127, 0.002, 0.012)
		blade.mesh = blade_mesh
		blade.material_override = _material(Color(0.08, 0.08, 0.08))
		pivot.add_child(blade)
		var disc := MeshInstance3D.new()
		var disc_mesh := CylinderMesh.new()
		disc_mesh.top_radius = 0.0635
		disc_mesh.bottom_radius = 0.0635
		disc_mesh.height = 0.001
		disc.mesh = disc_mesh
		var disc_material := _material(Color(1, 1, 1, 0.12))
		disc_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		disc.material_override = disc_material
		pivot.add_child(disc)


## Turns the propellers: `commands` are the motor commands in [0, 1], `delta` the frame time.
func set_motors(commands: PackedFloat32Array, delta: float) -> void:
	for i in mini(_props.size(), commands.size()):
		# Positive rotation about +Y is counter-clockwise seen from above, so clockwise motors turn negatively.
		_props[i].rotate_y(-SPIN[i] * commands[i] * DRAWN_SPIN * delta)


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.8
	return material


func _add_box(node_name: String, center: Vector3, size: Vector3, color: Color) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)
	return instance
```

`godot/drone/fpv_camera.gd`:

```gdscript
extends Camera3D
## The FPV camera on the drone: a wide horizontal field of view, tilted up so the horizon stays in view while the
## drone leans forward to fly fast. (Lens distortion and exposure are the `Lens` layer's job, see ui/lens.gd.)

## Horizontal field of view in degrees. A rectilinear projection gets very stretched at the edges above about 130.
@export_range(60.0, 160.0) var fov_h_deg := 110.0:
	set(value):
		fov_h_deg = value
		_apply()
## How far the camera is tilted up from the drone's forward axis, in degrees.
@export_range(0.0, 70.0) var uptilt_deg := 30.0:
	set(value):
		uptilt_deg = value
		_apply()


func _ready() -> void:
	near = 0.02
	far = 6000.0
	position = Vector3(0.0, 0.02, -0.04)  # just ahead of and above the drone's centre
	_apply()


func _apply() -> void:
	keep_aspect = Camera3D.KEEP_WIDTH  # `fov` is then the horizontal field of view
	fov = clampf(fov_h_deg, 1.0, 179.0)
	# Positive rotation about the camera's X axis pitches its view (-Z) up.
	rotation_degrees = Vector3(uptilt_deg, 0.0, 0.0)
```

`godot/drone/chase_camera.gd`:

```gdscript
extends Camera3D
## A third-person camera behind the drone, for finding out what the drone is doing when the FPV view is confusing.

@export var target_path: NodePath = ^"../Drone"
@export var distance := 1.2
@export var height := 0.45
@export var smoothing := 8.0


func _ready() -> void:
	near = 0.05
	far = 6000.0
	var target := get_node_or_null(target_path) as Node3D
	if target != null:
		global_position = _desired(target)
		_aim(target)


func _process(delta: float) -> void:
	var target := get_node_or_null(target_path) as Node3D
	if target == null:
		return
	global_position = global_position.lerp(_desired(target), 1.0 - exp(-smoothing * delta))
	_aim(target)


## Behind the drone along its heading (ignoring pitch and roll), a little above.
func _desired(target: Node3D) -> Vector3:
	var forward := -target.global_transform.basis.z
	forward.y = 0.0
	forward = Vector3(0, 0, -1) if forward.length() < 1e-3 else forward.normalized()
	return target.global_position - forward * distance + Vector3.UP * height


func _aim(target: Node3D) -> void:
	var look_at_point := target.global_position + Vector3.UP * 0.05
	if not global_position.is_equal_approx(look_at_point):
		look_at(look_at_point, Vector3.UP)
```

`godot/ui/lens.gdshader`:

```glsl
shader_type canvas_item;
// The FPV camera's lens: barrel distortion, exposure and a vignette, applied to everything drawn below this layer.
// The picture is zoomed just enough that the distorted image still fills the screen (no black corners).

uniform sampler2D screen_tex : hint_screen_texture, repeat_disable, filter_linear;
uniform float k1 = 0.06;
uniform float k2 = 0.0;
uniform float exposure = 1.0;
uniform float vignette = 0.25;

void fragment() {
	float aspect = SCREEN_PIXEL_SIZE.y / SCREEN_PIXEL_SIZE.x;
	vec2 p = (SCREEN_UV - 0.5) * 2.0;
	p.x *= aspect;
	float corner2 = aspect * aspect + 1.0;
	float fill = 1.0 / (1.0 + k1 * corner2 + k2 * corner2 * corner2);
	float r2 = dot(p, p);
	vec2 q = p * fill * (1.0 + k1 * r2 + k2 * r2 * r2);
	q.x /= aspect;
	vec2 uv = q * 0.5 + 0.5;
	vec3 color = texture(screen_tex, uv).rgb * exposure;
	color *= 1.0 - vignette * smoothstep(0.35, 1.0, r2 / corner2);
	COLOR = vec4(color, 1.0);
}
```

`godot/ui/lens.gd`:

```gdscript
extends CanvasLayer
## The FPV lens: a full-screen shader over the 3D view (barrel distortion, exposure, vignette). The HUD sits on a
## layer above this one, so it is not distorted. Hidden in the chase view.

const LENS_SHADER := preload("res://ui/lens.gdshader")

@export_range(-0.5, 0.8) var distortion := 0.06:
	set(value):
		distortion = value
		_apply()
@export_range(0.2, 3.0) var exposure := 1.0:
	set(value):
		exposure = value
		_apply()
@export_range(0.0, 1.0) var vignette := 0.25:
	set(value):
		vignette = value
		_apply()

var _material := ShaderMaterial.new()


func _ready() -> void:
	_material.shader = LENS_SHADER
	var rect := ColorRect.new()
	rect.name = "LensRect"
	rect.set_anchors_preset(Control.PRESET_FULL_RECT)
	rect.mouse_filter = Control.MOUSE_FILTER_IGNORE
	rect.material = _material
	add_child(rect)
	_apply()


func set_enabled(enabled: bool) -> void:
	visible = enabled


func _apply() -> void:
	_material.set_shader_parameter("k1", distortion)
	_material.set_shader_parameter("exposure", exposure)
	_material.set_shader_parameter("vignette", vignette)
```

- [ ] **Step 2: Verify the import stays clean.**

```bash
timeout -k 5 120 "$GODOT" --headless --path godot --import
```

Expected: exit 0, no `SCRIPT ERROR` / `Parse Error` lines. A parse error exits 1; a runtime error hangs and the timeout kills it — either way the step fails, which is the point.

Deeper verification of the visuals happens with the real renderer in Task 9 (`shots.gd`); the open-loop e2e exercises the drone and cameras headless.

- [ ] **Step 3: Commit** (including the `*.uid` files):

```bash
git add godot
git commit -m "feat(godot): the grey-box world, the drone and the FPV and chase cameras"
```

---

### Task 8: The HUD, the sticks view, the controls menu, and the app controller

**Files:**
- Create: `godot/ui/hud.gd`, `godot/ui/sticks_view.gd`, `godot/ui/controls_menu.gd`, `godot/scripts/app.gd`, `godot/main.tscn`, `godot/tests/test_hud.gd`, `godot/tests/test_controls_menu.gd`
- Modify: `godot/tests/run_tests.gd` (the final four-suite version)

**Interfaces:**
- Consumes: `OfsClient` (Task 5), `Controls` and `AppSettings` (Task 6), `World`/`Drone`/cameras (Task 7).
- Produces:
  - `Hud`: simulator status, radio link, battery, flight numbers (attempts/airtime/distance), the live sticks display, toasts and banners; `set_hud_visible`;
  - `SticksView`: the sticks being sent, drawn like a transmitter's (left box yaw/throttle, right box roll/pitch);
  - `ControlsMenu` (F2): choose a preset, bind each channel by moving its stick or key, invert, save;
  - `App` (`app.gd`, the `main.tscn` root): wires settings, client, input, world, drone, cameras, HUD and menus; the key handlers (pause, radio cut, reload, camera toggle, help F1, controls F2, quit).

- [ ] **Step 1: Create the files.** Each exactly:

`godot/ui/hud.gd`:

```gdscript
extends CanvasLayer
## The heads-up display: simulator status, radio link, battery, flight numbers, the sticks being sent, a banner
## for what needs attention, and short-lived toasts for events.
##
## `update_view` is called every frame with a Dictionary (see its comment); everything else is built here in code.

const SticksView = preload("res://ui/sticks_view.gd")

const GREEN := Color(0.45, 0.95, 0.5)
const YELLOW := Color(1.0, 0.85, 0.3)
const RED := Color(1.0, 0.38, 0.32)
const WHITE := Color(0.95, 0.97, 1.0)
const TOAST_SECONDS := 6.0
const MAX_TOASTS := 6
## No state for this long means the simulator stopped talking.
const STALE_SECONDS := 0.5

const HELP_LINES := [
	"Open FPV Sim",
	"P  pause / resume the simulation",
	"R  reload the quad (back to the start; retry after an error)",
	"C  switch between FPV and chase camera",
	"K  cut / restore the radio link (failsafe test)",
	"H  show / hide this HUD",
	"F1 this help        F2 controls setup        F11 fullscreen",
	"Arm with the aux 1 switch; aux 2 selects Angle mode.",
]

var _status := Label.new()
var _sim := Label.new()
var _link := Label.new()
var _battery := Label.new()
var _flight := Label.new()
var _configurator := Label.new()
var _banner := Label.new()
var _help := Label.new()
var _toasts := VBoxContainer.new()
var _sticks_view: Control = SticksView.new()
var _toast_items: Array = []  # of {label, age}
var _fatal := ""
var _shown := true


func _ready() -> void:
	var root := Control.new()
	root.name = "HudRoot"
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)
	_place(root, _status, 0.0, 0.0, 14.0, 14.0, false)
	_place(root, _sim, 0.0, 0.0, 14.0, 44.0, false)
	_place(root, _link, 1.0, 0.0, 14.0, 14.0, true)
	_place(root, _battery, 1.0, 0.0, 14.0, 44.0, true)
	_place(root, _flight, 0.0, 1.0, 14.0, 14.0, false)
	_place(root, _configurator, 1.0, 1.0, 14.0, 14.0, true)
	_place(root, _toasts, 1.0, 0.5, 14.0, 0.0, true)
	_place(root, _sticks_view, 0.5, 1.0, 0.0, 14.0, false)
	_sticks_view.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_banner.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_banner.add_theme_font_size_override("font_size", 30)
	_banner.add_theme_color_override("font_outline_color", Color.BLACK)
	_banner.add_theme_constant_override("outline_size", 8)
	_banner.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_banner.custom_minimum_size = Vector2(900, 0)
	_place(root, _banner, 0.5, 0.3, 0.0, 0.0, false)
	_banner.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.text = "\n".join(HELP_LINES)
	_help.visible = false
	_help.add_theme_font_size_override("font_size", 18)
	_help.add_theme_color_override("font_outline_color", Color.BLACK)
	_help.add_theme_constant_override("outline_size", 6)
	_place(root, _help, 0.5, 0.5, 0.0, 0.0, false)
	_help.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_help.grow_vertical = Control.GROW_DIRECTION_BOTH
	for label in [_status, _sim, _link, _battery, _flight, _configurator]:
		_style(label, 18)


func _process(delta: float) -> void:
	for item in _toast_items.duplicate():
		item["age"] += delta
		var left: float = TOAST_SECONDS - item["age"]
		if left <= 0.0:
			item["label"].queue_free()
			_toast_items.erase(item)
		else:
			item["label"].modulate.a = clampf(left, 0.0, 1.0)


## Places `control` at a fraction of the screen (`ax`, `ay`) with a margin; right and bottom anchored controls
## grow leftwards and upwards so their text stays on the screen.
func _place(parent: Control, control: Control, ax: float, ay: float, margin_x: float, margin_y: float, right_aligned: bool) -> void:
	parent.add_child(control)
	control.anchor_left = ax
	control.anchor_right = ax
	control.anchor_top = ay
	control.anchor_bottom = ay
	var mx := -margin_x if ax >= 1.0 else margin_x
	var my := -margin_y if ay >= 1.0 else margin_y
	control.offset_left = mx
	control.offset_right = mx
	control.offset_top = my
	control.offset_bottom = my
	control.grow_horizontal = Control.GROW_DIRECTION_BEGIN if ax >= 1.0 else Control.GROW_DIRECTION_END
	control.grow_vertical = Control.GROW_DIRECTION_BEGIN if ay >= 1.0 else Control.GROW_DIRECTION_END
	if control is Label and right_aligned:
		control.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	control.mouse_filter = Control.MOUSE_FILTER_IGNORE


func _style(label: Label, size: int) -> void:
	label.add_theme_font_size_override("font_size", size)
	label.add_theme_color_override("font_color", WHITE)
	label.add_theme_color_override("font_outline_color", Color.BLACK)
	label.add_theme_constant_override("outline_size", 5)


func set_help_visible(visible_now: bool) -> void:
	_help.visible = visible_now


func help_visible() -> bool:
	return _help.visible


func set_hud_visible(visible_now: bool) -> void:
	_shown = visible_now
	for node in [_status, _sim, _link, _battery, _flight, _configurator, _sticks_view, _toasts]:
		node.visible = visible_now


func toggle_hud() -> void:
	set_hud_visible(not _shown)


func hud_visible() -> bool:
	return _shown


## A message that stays until the next `update_view` without it, for failures that stop everything (the extension
## is missing).
func show_fatal(text: String) -> void:
	_fatal = text
	_banner.text = text
	_banner.add_theme_color_override("font_color", RED)


func add_toast(text: String, level: String = "info") -> void:
	var label := Label.new()
	label.text = text
	_style(label, 18)
	label.add_theme_color_override("font_color", {"info": WHITE, "warn": YELLOW, "error": RED}.get(level, WHITE))
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	_toasts.add_child(label)
	_toast_items.append({"label": label, "age": 0.0})
	while _toast_items.size() > MAX_TOASTS:
		var oldest: Dictionary = _toast_items.pop_front()
		oldest["label"].queue_free()


## The toasts on screen, oldest first (for the tests).
func toast_texts() -> PackedStringArray:
	var texts := PackedStringArray()
	for item in _toast_items:
		texts.append(item["label"].text)
	return texts


func banner_text() -> String:
	return _banner.text


## view: {phase, detail, telemetry: Dictionary (empty before the first state), sticks: Dictionary from Controls.read,
## controls_status, configurator, quad, camera, radio_cut}
func update_view(view: Dictionary) -> void:
	var t: Dictionary = view.get("telemetry", {})
	var phase: String = view.get("phase", "")
	_status.text = "%s  |  %s camera" % [view.get("quad", "") if view.get("quad", "") != "" else "Open FPV Sim", view.get("camera", "FPV")]
	if t.is_empty():
		_sim.text = phase.capitalize()
		_link.text = ""
		_battery.text = ""
		_flight.text = ""
	else:
		var running: bool = t["running"]
		_sim.text = "t = %.1f s   %s   overruns %d%s" % [
			t["time_s"], "REAL TIME" if running else "PAUSED", t["overruns"],
			"   Betaflight restarts %d" % t["fc_restarts"] if t["fc_restarts"] > 0 else ""]
		_sim.add_theme_color_override("font_color", WHITE if running else YELLOW)
		var up: bool = t["link_up"] and t["tx_enabled"]
		_link.text = "LINK %s   LQ %d %%   %d dBm" % ["UP" if up else "DOWN", t["lq_pct"], t["rssi_dbm"]]
		_link.add_theme_color_override("font_color", GREEN if up and t["lq_pct"] >= 80.0 else (YELLOW if up else RED))
		_battery.text = "%.2f V   %.1f A" % [t["battery_voltage_v"], t["battery_current_a"]]
		_flight.text = "ALT   %.1f m
SPEED %.1f m/s
CLIMB %+.1f m/s
MOTORS %s" % [
			t["altitude_m"], t["speed_mps"], t["climb_mps"], "ON" if t["motors_spinning"] else "off"]
	var sticks: Dictionary = view.get("sticks", {})
	if not sticks.is_empty():
		_sticks_view.sticks = sticks
	var address: String = view.get("configurator", "")
	_configurator.text = "Betaflight Configurator: %s" % address if address != "" else ""
	_update_banner(view, t, phase)


func _update_banner(view: Dictionary, t: Dictionary, phase: String) -> void:
	if _fatal != "":
		return
	var text := ""
	var color := YELLOW
	if phase == "failed":
		text = "%s\nPress R to retry." % view.get("detail", "")
		color = RED
	elif phase == "connecting" or phase == "loading":
		text = view.get("detail", "")
		if text == "":
			text = phase.capitalize() + "..."
	elif phase == "stopped":
		text = "Disconnected"
		color = RED
	elif not t.is_empty() and t["age_s"] > STALE_SECONDS:
		text = "NO DATA from the simulator (%.0f s)" % t["age_s"]
		color = RED
	elif view.get("radio_cut", false):
		text = "RADIO LINK CUT (K restores it)"
		color = RED
	elif not t.is_empty() and not t["link_up"] and t["tx_enabled"]:
		text = "RADIO LINK LOST - failsafe"
		color = RED
	elif view.get("controls_status", "") != "":
		text = view["controls_status"]
	elif phase == "paused":
		text = "PAUSED (P resumes)"
	_banner.text = text
	_banner.add_theme_color_override("font_color", color)
```

`godot/ui/sticks_view.gd`:

```gdscript
extends Control
## The sticks being sent, drawn like a transmitter's: the left box is yaw (across) and throttle (up), the right box
## roll (across) and pitch (up is stick forward), and four squares show the aux switches.
## Seeing it move is the quickest way to check that a controller is bound right.

const BOX := 84.0
const GAP := 14.0

var sticks: Dictionary = {}:
	set(value):
		sticks = value
		queue_redraw()


func _init() -> void:
	custom_minimum_size = Vector2(BOX * 2.0 + GAP * 3.0 + 60.0, BOX)
	mouse_filter = Control.MOUSE_FILTER_IGNORE


func _draw() -> void:
	if sticks.is_empty():
		return
	_stick_box(Rect2(0.0, 0.0, BOX, BOX), sticks["yaw"], sticks["throttle"] * 2.0 - 1.0)
	_stick_box(Rect2(BOX + GAP, 0.0, BOX, BOX), sticks["roll"], sticks["pitch"])
	var aux: Array = sticks["aux"]
	for i in aux.size():
		var rect := Rect2(BOX * 2.0 + GAP * 2.0 + (i % 2) * 28.0, (i / 2) * 28.0, 24.0, 24.0)
		var on: bool = aux[i] > 0.5
		draw_rect(rect, Color(0.3, 0.9, 0.4, 0.85) if on else Color(0, 0, 0, 0.4))
		draw_rect(rect, Color(1, 1, 1, 0.6), false, 1.5)


func _stick_box(rect: Rect2, x: float, y: float) -> void:
	draw_rect(rect, Color(0, 0, 0, 0.4))
	draw_rect(rect, Color(1, 1, 1, 0.6), false, 1.5)
	var c := rect.get_center()
	draw_line(Vector2(rect.position.x, c.y), Vector2(rect.end.x, c.y), Color(1, 1, 1, 0.2))
	draw_line(Vector2(c.x, rect.position.y), Vector2(c.x, rect.end.y), Color(1, 1, 1, 0.2))
	var dot := c + Vector2(clampf(x, -1.0, 1.0) * rect.size.x * 0.5, -clampf(y, -1.0, 1.0) * rect.size.y * 0.5)
	draw_circle(dot, 5.0, Color(1.0, 0.85, 0.3))
```

`godot/ui/controls_menu.gd`:

```gdscript
extends CanvasLayer
## The controls setup screen (F2): choose a preset, bind each channel by moving its stick or pressing its key or
## button, flip an axis, set the deadzone. Every change is saved at once. The live bars show what each channel is
## sending, so a wrong axis is obvious.

const STICK_CHANNELS := ["roll", "pitch", "yaw", "throttle"]
const LABELS := {
	"roll": "Roll", "pitch": "Pitch", "yaw": "Yaw", "throttle": "Throttle",
	"aux1": "Aux 1 (arm)", "aux2": "Aux 2 (angle mode)", "aux3": "Aux 3", "aux4": "Aux 4",
}
const BIND_THRESHOLD := 0.6

var controls = null  # Controls
var save_path := ""

var _panel := PanelContainer.new()
var _preset := OptionButton.new()
var _deadzone := HSlider.new()
var _deadzone_label := Label.new()
var _hint := Label.new()
var _rows := {}  # channel -> {description, bind, invert, live}
var _listening := ""


func _ready() -> void:
	visible = false
	var dim := ColorRect.new()
	dim.color = Color(0, 0, 0, 0.55)
	dim.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(dim)
	_panel.set_anchors_preset(Control.PRESET_CENTER)
	_panel.grow_horizontal = Control.GROW_DIRECTION_BOTH
	_panel.grow_vertical = Control.GROW_DIRECTION_BOTH
	var style := StyleBoxFlat.new()
	style.bg_color = Color(0.1, 0.11, 0.14, 0.96)
	style.set_corner_radius_all(8)
	style.set_content_margin_all(18)
	_panel.add_theme_stylebox_override("panel", style)
	add_child(_panel)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 10)
	_panel.add_child(box)
	var title := Label.new()
	title.text = "Controls"
	title.add_theme_font_size_override("font_size", 26)
	box.add_child(title)

	var preset_row := HBoxContainer.new()
	box.add_child(preset_row)
	var preset_label := Label.new()
	preset_label.text = "Preset"
	preset_row.add_child(preset_label)
	_preset.add_item("Radio (USB joystick, AETR)", 0)
	_preset.add_item("Gamepad (Mode 2)", 1)
	_preset.add_item("Keyboard", 2)
	_preset.item_selected.connect(_on_preset_selected)
	preset_row.add_child(_preset)

	var grid := GridContainer.new()
	grid.columns = 5
	grid.add_theme_constant_override("h_separation", 14)
	box.add_child(grid)
	for channel in ["roll", "pitch", "yaw", "throttle", "aux1", "aux2", "aux3", "aux4"]:
		var name_label := Label.new()
		name_label.text = LABELS[channel]
		name_label.custom_minimum_size.x = 150
		var description := Label.new()
		description.custom_minimum_size.x = 260
		var bind := Button.new()
		bind.text = "Bind"
		bind.pressed.connect(_start_listening.bind(channel))
		var invert := CheckBox.new()
		invert.text = "Invert"
		invert.toggled.connect(_on_invert_toggled.bind(channel))
		var live := ProgressBar.new()
		live.min_value = -1.0
		live.max_value = 1.0
		live.show_percentage = false
		live.custom_minimum_size = Vector2(160, 18)
		for node in [name_label, description, bind, invert, live]:
			grid.add_child(node)
		_rows[channel] = {"description": description, "bind": bind, "invert": invert, "live": live}

	var dz_row := HBoxContainer.new()
	box.add_child(dz_row)
	var dz_title := Label.new()
	dz_title.text = "Deadzone"
	dz_row.add_child(dz_title)
	_deadzone.min_value = 0.0
	_deadzone.max_value = 0.3
	_deadzone.step = 0.005
	_deadzone.custom_minimum_size.x = 220
	_deadzone.value_changed.connect(_on_deadzone_changed)
	dz_row.add_child(_deadzone)
	dz_row.add_child(_deadzone_label)

	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	box.add_child(_hint)


## Hands the screen the mapper it edits and where to save it.
func setup(mapper, path: String) -> void:
	controls = mapper
	save_path = path
	_refresh()


func toggle() -> void:
	_listening = ""
	visible = not visible
	_hint.text = "Bind: then move the stick (or press the button or key). Esc cancels, F2 closes."
	if visible:
		_refresh()


func is_open() -> bool:
	return visible


## The channel being bound ("" when none).
func listening_for() -> String:
	return _listening


## Feeds the live bars from the sticks being sent (the dictionary `Controls.read` returns).
func update_live(sticks: Dictionary) -> void:
	if not visible or sticks.is_empty():
		return
	var values := {
		"roll": sticks["roll"], "pitch": sticks["pitch"], "yaw": sticks["yaw"], "throttle": sticks["throttle"] * 2.0 - 1.0,
		"aux1": sticks["aux"][0], "aux2": sticks["aux"][1], "aux3": sticks["aux"][2], "aux4": sticks["aux"][3],
	}
	for channel in _rows:
		_rows[channel]["live"].value = values[channel]


func _refresh() -> void:
	if controls == null or _rows.is_empty():
		return
	_preset.selected = {"radio": 0, "gamepad": 1, "keyboard": 2}.get(controls.preset_name, -1)
	_deadzone.set_value_no_signal(controls.deadzone)
	_deadzone_label.text = "%.3f" % controls.deadzone
	for channel in _rows:
		var binding: Dictionary = controls.bindings.get(channel, {"kind": "none"})
		_rows[channel]["description"].text = controls.describe(channel)
		var is_axis: bool = binding.get("kind", "none") == "axis"
		_rows[channel]["invert"].disabled = not is_axis
		_rows[channel]["invert"].set_pressed_no_signal(is_axis and binding.get("invert", false))
		_rows[channel]["bind"].text = "Bind"


func _start_listening(channel: String) -> void:
	_listening = channel
	_rows[channel]["bind"].text = "..."
	_hint.text = "Binding %s: move its stick%s. Esc cancels." % [LABELS[channel], "" if channel in STICK_CHANNELS else ", or press its button or key"]


func _input(event: InputEvent) -> void:
	if not visible or _listening == "":
		return
	var binding := {}
	if event is InputEventKey and event.pressed and not event.echo:
		if event.physical_keycode == KEY_ESCAPE:
			_listening = ""
			_refresh()
			get_viewport().set_input_as_handled()
			return
		if not _listening in STICK_CHANNELS:
			binding = {"kind": "key_toggle", "key": event.physical_keycode}
	elif event is InputEventJoypadMotion and absf(event.axis_value) >= BIND_THRESHOLD:
		binding = {"kind": "axis", "device": _device_for(event.device), "index": event.axis, "invert": false}
	elif event is InputEventJoypadButton and event.pressed and not _listening in STICK_CHANNELS:
		binding = {"kind": "button", "device": _device_for(event.device), "index": event.button_index}
	if binding.is_empty():
		return
	controls.set_binding(_listening, binding)
	_listening = ""
	_save()
	_refresh()
	get_viewport().set_input_as_handled()


## One controller connected: "any" (-1) survives it being replugged under another id; several: bind the one used.
func _device_for(device: int) -> int:
	return -1 if Input.get_connected_joypads().size() <= 1 else device


func _on_preset_selected(index: int) -> void:
	controls.apply_preset(["radio", "gamepad", "keyboard"][index])
	_save()
	_refresh()


func _on_invert_toggled(pressed: bool, channel: String) -> void:
	var binding: Dictionary = controls.bindings.get(channel, {}).duplicate()
	if binding.get("kind", "none") != "axis":
		return
	binding["invert"] = pressed
	controls.set_binding(channel, binding)
	_save()
	_refresh()


func _on_deadzone_changed(value: float) -> void:
	controls.deadzone = value
	_deadzone_label.text = "%.3f" % value
	_save()


func _save() -> void:
	if save_path == "":
		return
	var file := FileAccess.open(save_path, FileAccess.WRITE)
	if file != null:
		file.store_string(controls.to_json())
```

`godot/scripts/app.gd`:

```gdscript
extends Node3D
## The pilot client. It connects to ofs-sim (starting it when a server binary is configured), sends the pilot's
## sticks, draws the drone from the simulator's state and shows the HUD. Keys are listed in ui/hud.gd.

const Controls = preload("res://scripts/controls.gd")
const AppSettings = preload("res://scripts/settings.gd")
const CONTROLS_PATH := "user://controls.json"

## What to try, by error kind, shown under the error message.
const TIPS := {
	"unavailable": "Is ofs-sim running? The game starts it itself when the `server_bin` setting points at it (docs/dev-setup.md).",
	"launch": "Build the server (cargo build -p ofs-sim) and set OFS_SIM_BIN, or check that its port is free.",
	"firmware": "Betaflight SITL did not start. Set OFS_SITL_LAUNCH (docs/dev-setup.md), or add `-- --open-loop` to fly without firmware.",
	"config": "The quad file is missing or wrong (setting quad_path, or OFS_QUAD).",
	"pilot_busy": "Another pilot (a script or a second game) is connected to this server.",
	"protocol": "The server and this game are different versions: rebuild both.",
	"not_loaded": "The session ended on the server.",
}

@onready var drone: Node3D = $Drone
@onready var fpv_camera: Camera3D = $Drone/FpvCamera
@onready var chase_camera: Camera3D = $ChaseCamera
@onready var lens: CanvasLayer = $Lens
@onready var hud: CanvasLayer = $Hud
@onready var controls_menu: CanvasLayer = $ControlsMenu

var client: Object = null  # OfsClient, from the extension
var controls = Controls.new()
var settings := {}
## Tests set this (a Dictionary like `Controls.read` returns) to fly without a controller.
var sticks_override = null
var radio_cut := false
var chase_view := false
var last_sticks := {}
var phase_kind := ""
var _link_was_lost := false


func _ready() -> void:
	settings = AppSettings.load_settings()
	_load_controls()
	controls_menu.setup(controls, CONTROLS_PATH)
	if not ClassDB.class_exists("OfsClient"):
		hud.show_fatal("The OfsClient extension is not loaded.\nBuild it with `cargo build -p ofs-godot` (docs/dev-setup.md) and restart.")
		set_process(false)
		return
	client = ClassDB.instantiate("OfsClient")
	client.name = "OfsClient"
	add_child(client)
	client.phase_changed.connect(_on_phase_changed)
	client.event_received.connect(_on_event)
	client.request_failed.connect(_on_request_failed)
	client.session_ready.connect(_on_session_ready)
	client.start(AppSettings.to_client_dict(settings))


func _load_controls() -> void:
	if FileAccess.file_exists(CONTROLS_PATH):
		var text := FileAccess.get_file_as_string(CONTROLS_PATH)
		if not controls.from_json(text):
			push_warning("%s is not a valid controls file; using the %s preset" % [CONTROLS_PATH, controls.preset_name])


func _process(delta: float) -> void:
	var focused := get_window().has_focus()
	last_sticks = sticks_override if sticks_override != null else controls.read(Input, focused, delta)
	client.set_sticks(last_sticks["roll"], last_sticks["pitch"], last_sticks["yaw"], last_sticks["throttle"], PackedFloat32Array(last_sticks["aux"]))
	if client.has_pose():
		drone.transform = client.get_pose()
	var telemetry: Dictionary = client.get_telemetry()
	if telemetry.has("motor_cmd"):
		drone.set_motors(telemetry["motor_cmd"], delta)
	var detail: String = client.get_phase_detail()
	if client.get_phase() == "failed" and TIPS.has(phase_kind):
		detail += "\n" + TIPS[phase_kind]
	controls_menu.update_live(last_sticks)
	hud.update_view({
		"phase": client.get_phase(), "detail": detail, "telemetry": telemetry, "sticks": last_sticks,
		"controls_status": last_sticks.get("status", ""), "configurator": client.get_configurator_address(),
		"quad": client.get_quad_name(), "camera": "Chase" if chase_view else "FPV", "radio_cut": radio_cut,
	})


func _unhandled_key_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	match event.physical_keycode:
		KEY_P:
			if client != null:
				client.resume() if client.get_phase() == "paused" else client.pause()
		KEY_R:
			if client != null:
				client.reload()
		KEY_C:
			set_chase_view(not chase_view)
		KEY_K:
			set_radio_cut(not radio_cut)
		KEY_H:
			hud.toggle_hud()
		KEY_F1:
			hud.set_help_visible(not hud.help_visible())
		KEY_F2:
			controls_menu.toggle()
		KEY_F11:
			var mode := DisplayServer.window_get_mode()
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED if mode == DisplayServer.WINDOW_MODE_FULLSCREEN else DisplayServer.WINDOW_MODE_FULLSCREEN)
		KEY_ESCAPE:
			hud.set_help_visible(false)
			if controls_menu.is_open() and controls_menu.listening_for() == "":
				controls_menu.toggle()


func set_chase_view(chase: bool) -> void:
	chase_view = chase
	(chase_camera if chase else fpv_camera).make_current()
	lens.set_enabled(not chase)


func set_radio_cut(cut: bool) -> void:
	radio_cut = cut
	if client != null:
		client.set_radio_loss(cut)


func _on_phase_changed(phase: String, detail: String, kind: String) -> void:
	phase_kind = kind
	if phase == "loading":
		radio_cut = false  # a reloaded quad has no faults
		_link_was_lost = false
	if phase == "failed":
		hud.add_toast("Failed: %s" % detail.get_slice("\n", 0), "error")


func _on_session_ready(quad_name: String, configurator_address: String) -> void:
	hud.add_toast("Loaded %s" % quad_name)
	if configurator_address != "":
		hud.add_toast("Betaflight Configurator: %s" % configurator_address)


func _on_request_failed(kind: String, message: String) -> void:
	hud.add_toast("%s: %s" % [kind, message], "warn")


func _on_event(kind: String, message: String, _time_s: float) -> void:
	match kind:
		"link_down":
			_link_was_lost = true
			hud.add_toast("Radio link lost - Betaflight fails safe", "warn")
		"link_up":
			hud.add_toast("Radio link restored" if _link_was_lost else "Radio link up")
			_link_was_lost = false
		"overrun":
			hud.add_toast("Real-time overrun: %s" % message, "warn")
		"firmware_restarted":
			hud.add_toast("Betaflight restarted: %s" % message)
		"sim_error":
			hud.add_toast("Simulator error: %s" % message, "error")
		"session_ended":
			hud.add_toast("Session ended: %s" % message, "warn")
		"pilot_connected", "pilot_disconnected":
			hud.add_toast(message)
		_:
			hud.add_toast("%s: %s" % [kind, message])


func _exit_tree() -> void:
	if client != null:
		client.stop()
```

`godot/main.tscn`:

```ini
[gd_scene format=3]

[ext_resource type="Script" path="res://scripts/app.gd" id="1"]
[ext_resource type="Script" path="res://world/world.gd" id="2"]
[ext_resource type="Script" path="res://drone/drone.gd" id="3"]
[ext_resource type="Script" path="res://drone/fpv_camera.gd" id="4"]
[ext_resource type="Script" path="res://drone/chase_camera.gd" id="5"]
[ext_resource type="Script" path="res://ui/lens.gd" id="6"]
[ext_resource type="Script" path="res://ui/hud.gd" id="7"]
[ext_resource type="Script" path="res://ui/controls_menu.gd" id="8"]

[node name="Main" type="Node3D"]
script = ExtResource("1")

[node name="World" type="Node3D" parent="."]
script = ExtResource("2")

[node name="Drone" type="Node3D" parent="."]
script = ExtResource("3")

[node name="FpvCamera" type="Camera3D" parent="Drone"]
current = true
script = ExtResource("4")

[node name="ChaseCamera" type="Camera3D" parent="."]
script = ExtResource("5")

[node name="Lens" type="CanvasLayer" parent="."]
layer = 1
script = ExtResource("6")

[node name="Hud" type="CanvasLayer" parent="."]
layer = 2
script = ExtResource("7")

[node name="ControlsMenu" type="CanvasLayer" parent="."]
layer = 3
script = ExtResource("8")
```

`godot/tests/test_hud.gd`:

```gdscript
extends "res://tests/testing.gd"

const Hud = preload("res://ui/hud.gd")


func _hud() -> CanvasLayer:
	var hud := Hud.new()
	await add_to_tree(hud)
	return hud


func _telemetry(overrides := {}) -> Dictionary:
	var t := {
		"time_s": 12.3, "altitude_m": 4.5, "speed_mps": 6.7, "climb_mps": -0.4, "battery_voltage_v": 24.1,
		"battery_current_a": 12.0, "motor_cmd": PackedFloat32Array([0.3, 0.3, 0.3, 0.3]), "motors_spinning": true,
		"tx_enabled": true, "link_up": true, "lq_pct": 100.0, "rssi_dbm": -50.0, "running": true, "overruns": 0,
		"fc_restarts": 0, "age_s": 0.01,
	}
	t.merge(overrides, true)
	return t


func _view(overrides := {}) -> Dictionary:
	var v := {"phase": "flying", "detail": "", "telemetry": _telemetry(), "sticks": {}, "controls_status": "",
		"configurator": "", "quad": "Test quad", "camera": "FPV", "radio_cut": false}
	v.merge(overrides, true)
	return v


func test_a_healthy_flight_has_no_banner() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	eq(hud.banner_text(), "", "no banner")
	hud.queue_free()


func test_each_problem_has_its_banner() -> void:
	var hud := await _hud()
	hud.update_view(_view({"phase": "failed", "detail": "no ofs-sim answers on 127.0.0.1:50051"}))
	ok(hud.banner_text().contains("no ofs-sim answers") and hud.banner_text().contains("R to retry"), "failed: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "loading", "detail": "loading quad.toml (Betaflight takes a few seconds to boot)", "telemetry": {}}))
	ok(hud.banner_text().begins_with("loading quad.toml"), "loading: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "connecting", "detail": "", "telemetry": {}}))
	eq(hud.banner_text(), "Connecting...", "connecting without a detail")
	hud.update_view(_view({"phase": "stopped", "telemetry": {}}))
	eq(hud.banner_text(), "Disconnected", "stopped")
	hud.update_view(_view({"telemetry": _telemetry({"age_s": 2.0})}))
	ok(hud.banner_text().begins_with("NO DATA"), "stale telemetry: %s" % hud.banner_text())
	hud.update_view(_view({"radio_cut": true}))
	ok(hud.banner_text().begins_with("RADIO LINK CUT"), "cut by the pilot: %s" % hud.banner_text())
	hud.update_view(_view({"telemetry": _telemetry({"link_up": false})}))
	ok(hud.banner_text().begins_with("RADIO LINK LOST"), "lost: %s" % hud.banner_text())
	hud.update_view(_view({"controls_status": "Controller not connected: the sticks are centred"}))
	ok(hud.banner_text().begins_with("Controller not connected"), "controller: %s" % hud.banner_text())
	hud.update_view(_view({"phase": "paused"}))
	ok(hud.banner_text().begins_with("PAUSED"), "paused: %s" % hud.banner_text())
	hud.queue_free()


func test_a_fatal_message_stays() -> void:
	var hud := await _hud()
	hud.show_fatal("The OfsClient extension is not loaded.")
	hud.update_view(_view())
	eq(hud.banner_text(), "The OfsClient extension is not loaded.", "the banner is not overwritten")
	hud.queue_free()


func test_toasts_are_limited_and_expire() -> void:
	var hud := await _hud()
	for i in 10:
		hud.add_toast("event %d" % i)
	eq(hud.toast_texts().size(), Hud.MAX_TOASTS, "only the newest are kept")
	eq(hud.toast_texts()[-1], "event 9", "newest last")
	eq(hud.toast_texts()[0], "event %d" % (10 - Hud.MAX_TOASTS), "oldest first")
	hud._process(Hud.TOAST_SECONDS + 0.1)
	eq(hud.toast_texts().size(), 0, "they fade away")
	hud.queue_free()


func test_the_hud_can_be_hidden_and_the_help_shown() -> void:
	var hud := await _hud()
	ok(hud.hud_visible(), "visible at first")
	hud.toggle_hud()
	ok(not hud.hud_visible(), "hidden")
	hud.toggle_hud()
	ok(hud.hud_visible(), "visible again")
	ok(not hud.help_visible(), "no help at first")
	hud.set_help_visible(true)
	ok(hud.help_visible(), "help shown")
	hud.queue_free()


func test_updates_cope_with_missing_data() -> void:
	var hud := await _hud()
	hud.update_view({})
	hud.update_view({"phase": "flying", "telemetry": {}})
	hud.update_view(_view({"sticks": {"roll": 0.1, "pitch": -0.2, "yaw": 0.0, "throttle": 0.5, "aux": [1.0, -1.0, -1.0, -1.0]}}))
	ok(true, "no errors")
	hud.queue_free()
```

`godot/tests/test_controls_menu.gd`:

```gdscript
extends "res://tests/testing.gd"

const Controls = preload("res://scripts/controls.gd")
const Menu = preload("res://ui/controls_menu.gd")

const SAVE_PATH := "user://test_controls.json"


func _menu(preset := "radio") -> Array:
	var controls := Controls.new(preset)
	var menu := Menu.new()
	await add_to_tree(menu)
	DirAccess.remove_absolute(SAVE_PATH)
	menu.setup(controls, SAVE_PATH)
	menu.toggle()
	return [menu, controls]


func _axis_event(device: int, axis: int, value: float) -> InputEventJoypadMotion:
	var e := InputEventJoypadMotion.new()
	e.device = device
	e.axis = axis
	e.axis_value = value
	return e


func _key_event(keycode: int) -> InputEventKey:
	var e := InputEventKey.new()
	e.physical_keycode = keycode
	e.pressed = true
	return e


func test_moving_a_stick_binds_the_channel_and_saves() -> void:
	var m := await _menu()
	var menu = m[0]
	var controls = m[1]
	menu._start_listening("roll")
	eq(menu.listening_for(), "roll", "listening")
	menu._input(_axis_event(0, 5, 0.2))
	eq(menu.listening_for(), "roll", "a small movement is ignored")
	menu._input(_axis_event(0, 5, -0.9))
	eq(menu.listening_for(), "", "bound")
	eq(controls.bindings["roll"]["kind"], "axis", "an axis binding")
	eq(controls.bindings["roll"]["index"], 5, "axis 5")
	eq(controls.bindings["roll"]["device"], -1, "any controller when at most one is connected")
	eq(controls.preset_name, "custom", "no longer a preset")
	var saved := Controls.new("keyboard")
	ok(saved.from_json(FileAccess.get_file_as_string(SAVE_PATH)), "the saved file loads")
	eq(saved.bindings["roll"], controls.bindings["roll"], "and holds the new binding")
	menu.queue_free()


func test_sticks_take_axes_only_and_switches_take_buttons_and_keys() -> void:
	var m := await _menu("gamepad")
	var menu = m[0]
	var controls = m[1]
	var before: Dictionary = controls.bindings["pitch"].duplicate()
	menu._start_listening("pitch")
	menu._input(_key_event(KEY_X))
	var button := InputEventJoypadButton.new()
	button.pressed = true
	button.button_index = 4
	menu._input(button)
	eq(controls.bindings["pitch"], before, "keys and buttons do not bind a stick")
	eq(menu.listening_for(), "pitch", "still listening")
	menu._input(_key_event(KEY_ESCAPE))
	eq(menu.listening_for(), "", "escape cancels")
	menu._start_listening("aux3")
	menu._input(button)
	eq(controls.bindings["aux3"]["kind"], "button", "a switch can take a button")
	eq(controls.bindings["aux3"]["index"], 4, "button 4")
	menu._start_listening("aux4")
	menu._input(_key_event(KEY_X))
	eq(controls.bindings["aux4"], {"kind": "key_toggle", "key": KEY_X}, "or a key")
	menu.queue_free()


func test_invert_preset_and_deadzone_change_the_controls_and_save() -> void:
	var m := await _menu("radio")
	var menu = m[0]
	var controls = m[1]
	menu._on_invert_toggled(true, "pitch")
	eq(controls.bindings["pitch"]["invert"], true, "inverted")
	menu._on_invert_toggled(true, "aux1")
	menu._on_preset_selected(1)
	eq(controls.preset_name, "gamepad", "the preset changes the bindings")
	eq(controls.bindings["throttle"]["invert"], true, "gamepad throttle is inverted by the preset")
	menu._on_deadzone_changed(0.12)
	near(controls.deadzone, 0.12, "deadzone")
	var saved := Controls.new("radio")
	ok(saved.from_json(FileAccess.get_file_as_string(SAVE_PATH)), "saved")
	near(saved.deadzone, 0.12, "the saved deadzone")
	eq(saved.preset_name, "gamepad", "the saved preset")
	menu.queue_free()


func test_the_live_bars_follow_the_sticks_only_while_open() -> void:
	var m := await _menu()
	var menu = m[0]
	var sticks := {"roll": 0.5, "pitch": -0.5, "yaw": 0.0, "throttle": 1.0, "aux": [1.0, -1.0, -1.0, -1.0]}
	menu.update_live(sticks)
	near(menu._rows["roll"]["live"].value, 0.5, "roll bar")
	near(menu._rows["throttle"]["live"].value, 1.0, "throttle bar (0..1 shown as -1..1)")
	near(menu._rows["aux1"]["live"].value, 1.0, "aux bar")
	menu.toggle()
	ok(not menu.is_open(), "closed")
	menu.update_live({"roll": -1.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [-1.0, -1.0, -1.0, -1.0]})
	near(menu._rows["roll"]["live"].value, 0.5, "a closed screen does not update")
	menu.queue_free()
```

Replace `godot/tests/run_tests.gd` with the final four-suite version:

```gdscript
extends SceneTree
## Runs the unit tests: godot --headless --path godot -s res://tests/run_tests.gd
## Exit code 0 when every check passed. (Run `godot --headless --path godot --import` once first.)

const SUITES := [
	"res://tests/test_controls.gd",
	"res://tests/test_settings.gd",
	"res://tests/test_hud.gd",
	"res://tests/test_controls_menu.gd",
]


func _initialize() -> void:
	_run()


func _run() -> void:
	var failures := 0
	var checks := 0
	for path in SUITES:
		var suite = load(path).new()
		suite.tree = self
		var failed: int = await suite.run_all()
		print("%s: %d checks, %d failed" % [path, suite.checks, failed])
		failures += failed
		checks += suite.checks
	print("TOTAL: %d checks, %d failed" % [checks, failures])
	quit(1 if failures > 0 else 0)
```

- [ ] **Step 2: Run the suites.**

```bash
timeout -k 5 120 "$GODOT" --headless --path godot --import
timeout -k 5 120 "$GODOT" --headless --path godot -s res://tests/run_tests.gd
```

Expected:
- `res://tests/test_controls.gd: 76 checks, 0 failed`
- `res://tests/test_settings.gd: 20 checks, 0 failed`
- `res://tests/test_hud.gd: 21 checks, 0 failed`
- `res://tests/test_controls_menu.gd: 27 checks, 0 failed`
- `TOTAL: 144 checks, 0 failed`, exit 0.

- [ ] **Step 3: Repeat three times** (three clean `TOTAL: 144 checks, 0 failed`).

- [ ] **Step 4: Commit:**

```bash
git add godot
git commit -m "feat(godot): HUD, sticks view, controls menu and the app controller"
```

---

### Task 9: End-to-end tests — open loop, and real Betaflight

**Files:**
- Create: `godot/tests/e2e_open_loop.gd`, `godot/tests/e2e_betaflight.gd`, `godot/tests/shots.gd`

**What they cover:**
- `e2e_open_loop.gd`: the game scene (`main.tscn`) against a real `ofs-sim` process with the open-loop FC, on a free port, in a temp data dir — connects (starting the server itself), reaches the flying phase, flies with the throttle climb, pauses, cuts and restores the radio (link events on the HUD), reloads, survives the server being killed, and disconnects cleanly. Prints `E2E PASSED` and exits 0.
- `e2e_betaflight.gd`: the same scene with real Betaflight SITL (needs `OFS_SITL_LAUNCH`, `docs/dev-setup.md`; skips without it) — armed by the simulated sticks through the ELRS/CRSF link, climbs in Angle mode, a reload re-boots Betaflight and re-arms.
- `shots.gd`: starts the app open-loop and captures full-res screenshots (`start_fpv`, `start_chase`, `hop_fpv`, `hop_chase`, `help`, `controls`) into `shots/` for human review; needs a GPU, never runs in CI.

- [ ] **Step 1: Create the files.** Each exactly:

`godot/tests/e2e_open_loop.gd`:

```gdscript
extends SceneTree
## End to end without Betaflight: the game scene starts a real ofs-sim (open loop), flies it and reacts to the
## radio being cut. Needs the extension and the server built (target/debug), or OFS_SIM_BIN set.
##   godot --headless --path godot -s res://tests/e2e_open_loop.gd

var failures := 0


func _initialize() -> void:
	_run()


func _check(condition: bool, message: String) -> void:
	if condition:
		print("  ok   ", message)
	else:
		failures += 1
		printerr("  FAIL ", message)


func _wait_for(condition: Callable, seconds: float) -> bool:
	var deadline := Time.get_ticks_msec() + int(seconds * 1000.0)
	while Time.get_ticks_msec() < deadline:
		if condition.call():
			return true
		await process_frame
	return condition.call()


func _press(app: Node, keycode: int) -> void:
	var event := InputEventKey.new()
	event.physical_keycode = keycode
	event.pressed = true
	app._unhandled_key_input(event)


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _run() -> void:
	var data_dir := OS.get_user_data_dir().path_join("e2e-data")
	OS.set_environment("OFS_OPEN_LOOP", "1")
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", data_dir)
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame  # the scene's _ready runs on the first frame
	_check(app.client != null, "the OfsClient extension is loaded and started")
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 30.0)
	_check(flying, "reaches the flying phase (phase: %s %s)" % [app.client.get_phase(), app.client.get_phase_detail()])
	_check(app.client.get_quad_name().contains("OpenDrone"), "the quad name is known: %s" % app.client.get_quad_name())
	_check(app.hud.toast_texts().size() > 0 and app.hud.toast_texts()[0].begins_with("Loaded"), "the HUD announces the quad: %s" % str(app.hud.toast_texts()))

	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.9, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var climbed: bool = await _wait_for(func(): return app.drone.position.y > 0.5, 20.0)
	_check(climbed, "the drone climbs under the sticks: y = %.2f" % app.drone.position.y)
	var telemetry: Dictionary = app.client.get_telemetry()
	_check(not telemetry.is_empty() and telemetry["tx_enabled"] and telemetry["link_up"] and telemetry["running"], "telemetry shows a live link: %s" % str(telemetry))
	_check(absf(app.drone.position.y - telemetry["altitude_m"]) < 1.0, "the drawn height follows the simulated altitude")
	_check(app.hud.banner_text() == "", "no banner while all is well: '%s'" % app.hud.banner_text())

	# Keys, through the game's own handler.
	_press(app, KEY_K)
	_check(app.radio_cut, "K cuts the radio")
	var told: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("Radio link lost"), 15.0)
	_check(told, "cutting the radio raises a link-lost toast: %s" % str(app.hud.toast_texts()))
	var banner: bool = await _wait_for(func(): return app.hud.banner_text().begins_with("RADIO LINK"), 5.0)
	_check(banner, "and a banner: '%s'" % app.hud.banner_text())
	_press(app, KEY_K)
	_check(not app.radio_cut, "K restores it")
	var restored: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("restored"), 15.0)
	_check(restored, "and the HUD says so")

	_press(app, KEY_P)
	var paused: bool = await _wait_for(func(): return app.client.get_phase() == "paused" and not app.client.get_telemetry()["running"], 5.0)
	_check(paused, "P pauses the simulation")
	var frozen_at: float = app.client.get_telemetry()["time_s"]
	await _wait_for(func(): return false, 0.3)
	_check(app.client.get_telemetry()["time_s"] == frozen_at, "simulated time stands still")
	_press(app, KEY_P)
	var resumed: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry()["time_s"] > frozen_at + 0.1, 5.0)
	_check(resumed, "P resumes it")

	_press(app, KEY_C)
	await process_frame
	_check(app.chase_camera.current and not app.lens.visible, "C: the chase camera takes over and the lens is off")
	_press(app, KEY_C)
	await process_frame
	_check(app.fpv_camera.current and app.lens.visible, "C again: back to FPV")
	_press(app, KEY_F1)
	_check(app.hud.help_visible(), "F1 shows the help")
	_press(app, KEY_ESCAPE)
	_check(not app.hud.help_visible(), "Escape hides it")
	_press(app, KEY_F2)
	_check(app.controls_menu.is_open(), "F2 opens the controls screen")
	_press(app, KEY_ESCAPE)
	_check(not app.controls_menu.is_open(), "Escape closes it")
	_press(app, KEY_H)
	_check(not app.hud.hud_visible(), "H hides the HUD")
	_press(app, KEY_H)
	_check(app.hud.hud_visible(), "and shows it again")

	var before_reload: float = app.client.get_telemetry()["time_s"]
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.0, "aux": [-1.0, -1.0, -1.0, -1.0], "status": ""}
	_press(app, KEY_R)
	var reloaded: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry().get("time_s", 1e9) < before_reload, 30.0)
	_check(reloaded, "R reloads the quad: simulated time restarts (%.1f s -> %.1f s)" % [before_reload, app.client.get_telemetry().get("time_s", -1.0)])
	await _wait_for(func(): return false, 0.3)
	_check(app.drone.position.y < 0.2, "and the drone is back on the ground: y = %.2f" % app.drone.position.y)
	_check(app.client.get_telemetry()["tx_enabled"], "the pilot link is back")

	app.queue_free()
	await process_frame
	await process_frame
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
```

`godot/tests/e2e_betaflight.gd`:

```gdscript
extends SceneTree
## End to end with real Betaflight SITL: the game scene starts ofs-sim, Betaflight boots, the sticks arm it through
## the simulated ExpressLRS link, it flies in Angle mode, and cutting the radio makes it fail safe (disarm).
## Needs the extension and the server built, and OFS_SITL_LAUNCH set (docs/dev-setup.md); skips without it.
##   godot --headless --path godot -s res://tests/e2e_betaflight.gd

var failures := 0


func _initialize() -> void:
	_run()


func _check(condition: bool, message: String) -> void:
	if condition:
		print("  ok   ", message)
	else:
		failures += 1
		printerr("  FAIL ", message)


func _wait_for(condition: Callable, seconds: float) -> bool:
	var deadline := Time.get_ticks_msec() + int(seconds * 1000.0)
	while Time.get_ticks_msec() < deadline:
		if condition.call():
			return true
		await process_frame
	return condition.call()


func _press(app: Node, keycode: int) -> void:
	var event := InputEventKey.new()
	event.physical_keycode = keycode
	event.pressed = true
	app._unhandled_key_input(event)


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _sticks(throttle: float, armed: bool) -> Dictionary:
	return {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": throttle, "aux": [1.0 if armed else -1.0, 1.0, -1.0, -1.0], "status": ""}


func _run() -> void:
	if not OS.has_environment("OFS_SITL_LAUNCH"):
		print("SKIP: OFS_SITL_LAUNCH is not set, so there is no Betaflight SITL to fly")
		quit(0)
		return
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", OS.get_user_data_dir().path_join("e2e-betaflight-data"))
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await process_frame
	app.sticks_override = _sticks(0.0, false)
	var flying: bool = await _wait_for(func(): return app.client.get_phase() == "flying", 90.0)
	_check(flying, "reaches the flying phase: %s %s" % [app.client.get_phase(), app.client.get_phase_detail()])
	_check(app.client.get_configurator_address().begins_with("tcp://"), "the Configurator address is offered: %s" % app.client.get_configurator_address())

	# Betaflight needs a few seconds after boot before it accepts arming.
	var booted: bool = await _wait_for(func(): return app.client.get_telemetry().get("time_s", 0.0) > 6.0, 30.0)
	_check(booted, "Betaflight has been running for 6 s of simulated time")
	app.sticks_override = _sticks(0.0, true)
	var armed: bool = await _wait_for(func(): return app.client.get_telemetry().get("motors_spinning", false), 10.0)
	_check(armed, "the arm switch arms Betaflight (motors idle): %s" % str(app.client.get_telemetry().get("motor_cmd")))

	app.sticks_override = _sticks(0.6, true)
	var climbed: bool = await _wait_for(func(): return app.drone.position.y > 2.0, 20.0)
	_check(climbed, "it climbs: y = %.2f" % app.drone.position.y)
	var up: float = app.drone.global_transform.basis.y.dot(Vector3.UP)
	_check(up > 0.9, "and Angle mode keeps it level (up vector . vertical = %.3f)" % up)
	var telemetry: Dictionary = app.client.get_telemetry()
	_check(telemetry["link_up"] and telemetry["tx_enabled"] and telemetry["running"], "the radio link is up")
	_check(telemetry["overruns"] == 0, "no real-time overruns: %d" % telemetry["overruns"])

	app.set_radio_cut(true)
	var told: bool = await _wait_for(func(): return " ".join(app.hud.toast_texts()).contains("Radio link lost"), 10.0)
	_check(told, "cutting the radio is reported")
	var disarmed: bool = await _wait_for(func(): return not app.client.get_telemetry().get("motors_spinning", true), 6.0)
	_check(disarmed, "Betaflight fails safe: the motors stop within 6 s of the cut")
	_press(app, KEY_K)  # the pilot's own failsafe test works through the key too
	_press(app, KEY_K)

	# Reload: Betaflight is stopped and started again, and the pilot link comes back.
	app.sticks_override = _sticks(0.0, false)
	var before_reload: float = app.client.get_telemetry()["time_s"]
	_press(app, KEY_R)
	var reloaded: bool = await _wait_for(func(): return app.client.get_phase() == "flying" and app.client.get_telemetry().get("time_s", 1e9) < before_reload, 60.0)
	_check(reloaded, "R reloads: Betaflight restarts and simulated time restarts")
	var rearmed_ok: bool = await _wait_for(func(): return app.client.get_telemetry().get("time_s", 0.0) > 6.0, 30.0)
	app.sticks_override = _sticks(0.0, true)
	var armed_again: bool = await _wait_for(func(): return app.client.get_telemetry().get("motors_spinning", false), 10.0)
	_check(rearmed_ok and armed_again, "and it arms again")

	app.queue_free()
	await process_frame
	await process_frame
	print("E2E %s" % ("PASSED" if failures == 0 else "FAILED (%d)" % failures))
	quit(1 if failures > 0 else 0)
```

`godot/tests/shots.gd`:

```gdscript
extends SceneTree
## Renders the game to PNG files with a real renderer (a window opens briefly), to check the visuals by eye:
##   godot --path godot -s res://tests/shots.gd -- --out=C:/some/dir
## Flies the open-loop drone forward for a moment, then saves fpv.png, chase.png, help.png and controls.png.

func _initialize() -> void:
	_run()


func _out_dir() -> String:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--out="):
			return arg.substr(6)
	return OS.get_user_data_dir()


func _free_port() -> int:
	var server := TCPServer.new()
	server.listen(0, "127.0.0.1")
	var port := server.get_local_port()
	server.stop()
	return port


func _frames(count: int) -> void:
	for i in count:
		await process_frame


func _save(name: String) -> void:
	await _frames(3)
	var image := root.get_viewport().get_texture().get_image()
	var path := _out_dir().path_join(name + ".png")
	print("saved ", path, " ", image.get_size(), " ", image.save_png(path))


func _run() -> void:
	OS.set_environment("OFS_OPEN_LOOP", "1")
	OS.set_environment("OFS_SERVER_ADDR", "127.0.0.1:%d" % _free_port())
	OS.set_environment("OFS_DATA_DIR", OS.get_user_data_dir().path_join("shots-data"))
	var app = load("res://main.tscn").instantiate()
	root.add_child(app)
	await _frames(2)
	var deadline := Time.get_ticks_msec() + 30000
	while app.client.get_phase() != "flying" and Time.get_ticks_msec() < deadline:
		await process_frame
	await _save("start_fpv")
	app.set_chase_view(true)
	await _frames(20)
	await _save("start_chase")
	app.set_chase_view(false)
	# A short low hop so the picture moves: climb a little, then let the drone settle forward.
	app.sticks_override = {"roll": 0.0, "pitch": 0.0, "yaw": 0.0, "throttle": 0.32, "aux": [1.0, -1.0, -1.0, -1.0], "status": ""}
	var until := Time.get_ticks_msec() + 1200
	while Time.get_ticks_msec() < until:
		await process_frame
	await _save("hop_fpv")
	app.set_chase_view(true)
	await _frames(20)
	await _save("hop_chase")
	app.set_chase_view(false)
	app.hud.set_help_visible(true)
	await _save("help")
	app.hud.set_help_visible(false)
	app.controls_menu.toggle()
	await _save("controls")
	app.queue_free()
	await _frames(2)
	quit(0)
```

- [ ] **Step 2: The open-loop e2e, three times.**

```bash
for i in 1 2 3; do timeout -k 5 180 "$GODOT" --headless --path godot -s res://tests/e2e_open_loop.gd; done
```

Expected: `E2E PASSED` three times, exit 0 each.

- [ ] **Step 3: The Betaflight e2e.** With the WSL SITL set up per `docs/dev-setup.md`:

```bash
# Windows (SITL in WSL Ubuntu):
export OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"
timeout -k 5 300 "$GODOT" --headless --path godot -s res://tests/e2e_betaflight.gd
```

Expected: not `SKIP:` — the scene reaches the flying phase, the quad name is known (`OpenDrone 5F freestyle …`), the Configurator address is offered (`tcp://127.0.0.1:<port>`), Betaflight runs ≥ 6 s of simulated time, the reload re-boots Betaflight and re-arms, and it prints `E2E PASSED`.

Without `OFS_SITL_LAUNCH` the test prints `SKIP: …` and exits 0 — CI relies on that.

- [ ] **Step 4: Human visual check (a GPU machine).**

```bash
"$GODOT" --path godot -s res://tests/shots.gd
```

Expected: `shots/start_fpv.png`, `start_chase.png`, `hop_fpv.png`, `hop_chase.png`, `help.png`, `controls.png` (1280×720). Review: the grid reads at altitude, the horizon sits where an FPV cam would put it, the props turn, the HUD and sticks draw, the help and controls screens lay out without overlap. Watch for Windows-only surprises: CR characters inside multi-line label strings render as extra line breaks (Decision 11) — the help text is built from a joined list for exactly this reason.

- [ ] **Step 5: Commit:**

```bash
git add godot
git commit -m "test(godot): open-loop and real-Betaflight end-to-end tests"
```

---

### Task 10: CI — the Godot test runner and the workflow job

**Files:**
- Create: `scripts/run-godot-tests.sh`
- Modify: `.github/workflows/ci.yml`

**Why the timeout matters (verified on 4.7.2):** a test script with a parse error exits 1; a script that raises a runtime error, or never quits, leaves the engine running forever. The runner wraps every Godot invocation in `timeout -k` and fails on a kill. `run_tests.gd` itself exits 1 when any check failed.

- [ ] **Step 1: Create the runner.** `scripts/run-godot-tests.sh` — exactly:

```bash
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

# One import so Godot has indexed the project before any run.
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
```

- [ ] **Step 2: Verify the runner locally.**

```bash
export GODOT_BIN=/path/to/Godot_v4.7.2-stable_win64_console.exe   # or let it download on Linux
bash scripts/run-godot-tests.sh unit
bash scripts/run-godot-tests.sh e2e
```

Expected:
- `unit` ends with `TOTAL: 144 checks, 0 failed` and `Godot tests passed.`;
- `e2e` ends with `E2E PASSED` and (without `OFS_SITL_LAUNCH`) the Betaflight skip line, then `Godot tests passed.`;
- without `GODOT_BIN`, the script downloads Godot 4.7.2 into `build/godot-dl/` and verifies it against the release's `SHA512-SUMS.txt` before using it.

- [ ] **Step 3: Prove the failure path.** A runtime-error script must fail a run:

```bash
timeout -k 5 15 "$GODOT_BIN" --headless --path godot -s res://tests/bad_runtime.gd; echo "rc=$?"
```

Expected: rc=124 (killed). With a parse-error script (`bad_parse.gd`): rc=1. Both fixtures were used during planning and are not committed; recreate them if you want to re-check (a dictionary read of a missing key; an empty `_initialize`).

- [ ] **Step 4: Wire the workflow.** Replace `.github/workflows/ci.yml` with exactly (the new `godot` job; the msrv job now skips `ofs-godot`, which alone needs Rust 1.94):

```yaml
name: ci
on: [push, pull_request]

jobs:
  core:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v6
        with:
          python-version: "3.12"
      - run: cargo test --workspace --locked
      - run: cargo build -p ofs-sim --locked
      - run: python -m pip install -e "python[dev]"
      - run: python -m pytest python/tests -v

  msrv:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@1.85
      # ofs-godot alone needs a newer Rust (godot-rust 0.5.5); the stable jobs cover it.
      - run: cargo check --workspace --all-targets --locked --exclude ofs-godot

  windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v6
        with:
          python-version: "3.12"
      - run: cargo test --workspace --locked
      - run: cargo build -p ofs-sim --locked
      - run: python -m pip install -e "python[dev]"
      # Without OFS_SITL_LAUNCH the SITL tests skip (no WSL SITL on the runner).
      - run: python -m pytest python/tests -v

  godot:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      # Builds ofs-sim + ofs-godot, downloads Godot 4.7.2 (SHA-512 verified),
      # runs the unit suites and the open-loop e2e; the Betaflight e2e skips.
      - run: bash scripts/run-godot-tests.sh all

  sitl:
    runs-on: ubuntu-24.04
    env:
      BF_DIR: ${{ github.workspace }}/../betaflight
      OFS_SITL_LAUNCH: ${{ github.workspace }}/../betaflight/obj/main/betaflight_SITL.elf
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v6
        with:
          python-version: "3.12"
      - run: bash scripts/build-sitl.sh
      - run: cargo test -p ofs-fc -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1
      - run: cargo build -p ofs-sim --locked
      - run: python -m pip install -e "python[dev]"
      - run: python -m pytest python/tests/test_sitl_hover.py python/tests/test_sitl_radio.py -v
```

- [ ] **Step 5: Commit:**

```bash
git add scripts/run-godot-tests.sh .github/workflows/ci.yml
git commit -m "ci: run the Godot headless tests; skip ofs-godot in the msrv job"
```

- [ ] **Step 6: Not pushed here.** Per the M2a practice, pushing `main` and watching the CI jobs is surfaced to the user at the end, not done unilaterally by an implementer.

---

### Task 11: Docs, and the Python `PilotBusy` mapping

The M2a ledger deferred one item because it needed this milestone: `python/ofs/errors.py` maps 7 of the 9 `ofs-error-kind` values; `pilot_busy` and `internal` fall through to the generic `OfsError`. M2b introduces a second pilot-bearing client, so a script pilot meeting a busy session deserves the typed error.

**Files:**
- Modify: `python/ofs/errors.py`, `python/ofs/__init__.py`, `python/tests/test_client.py`, `README.md`, `docs/dev-setup.md`

- [ ] **Step 1: Map the two kinds.** In `python/ofs/errors.py`, add after `ServerUnavailable`:

```python
class PilotBusy(OfsError):
    """Another pilot is already connected to the session."""


class InternalError(OfsError):
    """The server reported an internal error."""
```

and extend the `_KINDS` dict:

```python
    "pilot_busy": PilotBusy,
    "internal": InternalError,
```

In `python/ofs/__init__.py`, import `InternalError, PilotBusy` in the `from .errors import (...)` list and add both to `__all__`.

- [ ] **Step 2: Test the mapping.** Append to `python/tests/test_client.py`:

```python
def test_pilot_busy_and_internal_are_typed():
    import grpc

    from ofs.errors import from_rpc_error

    class FakeRpcError(grpc.RpcError):
        def __init__(self, kind, message):
            super().__init__(message)
            self._md = (("ofs-error-kind", kind),)
            self._message = message

        def trailing_metadata(self):
            return self._md

        def details(self):
            return self._message

        def code(self):
            return grpc.StatusCode.UNKNOWN

    assert isinstance(from_rpc_error(FakeRpcError("pilot_busy", "another pilot is flying")), ofs.PilotBusy)
    assert isinstance(from_rpc_error(FakeRpcError("internal", "boom")), ofs.InternalError)
```

Run: `python -m pytest python/tests/test_client.py -v` — the new test passes, nothing else changes.

- [ ] **Step 3: Document the client.** In `README.md`, extend the clients section: the Godot pilot client (what it is, how to run it: `godot --path godot` from the repo root after `cargo build -p ofs-sim -p ofs-godot`, F1 help, F2 controls, the settings precedence, `OFS_SIM_BIN`/`OFS_SERVER_ADDR`/`OFS_DATA_DIR` overrides, and that it flies through the ELRS/CRSF link into real Betaflight). Soften any Configurator wording that still says "should connect" only if the user's manual check has happened (see `docs/research/sitl-interface.md` §8).

In `docs/dev-setup.md`, add a "Godot pilot client" section: download Godot 4.7.2 (official zip, check it against the release's `SHA512-SUMS.txt`), the console binary for headless runs, `bash scripts/run-godot-tests.sh` (or `GODOT_BIN` pointing at any 4.7 binary), the e2e commands with and without `OFS_SITL_LAUNCH`, and `shots.gd` for the visual check. Note the `timeout -k` rationale (runtime script errors hang the engine) and that the extension loads from `target/`, so keep `CARGO_TARGET_DIR` at its default when running Godot.

- [ ] **Step 4: Test the Python side.**

```bash
python -m pytest python/tests -v
```

Expected: the suites pass with the new test added (14 passed, 3 skipped — the previous count was 13 passed, 3 skipped).

- [ ] **Step 5: Commit:**

```bash
git add python README.md docs/dev-setup.md
git commit -m "docs: the Godot pilot client; python maps pilot_busy and internal errors"
```

---

## Final verification (the controller runs this before the final review)

1. `cargo test --workspace --locked` on Windows — every binary green, no warnings.
2. Native Linux (WSL Ubuntu), from the worktree: `cargo test --workspace --locked` and `cargo build -p ofs-godot` — green; `libofs_godot.so` builds.
3. `python -m pytest python/tests -v` — 14 passed, 3 skipped.
4. `bash scripts/run-godot-tests.sh all` — `TOTAL: 144 checks, 0 failed`, then `E2E PASSED`.
5. With `OFS_SITL_LAUNCH` set: `godot --headless --path godot -s res://tests/e2e_betaflight.gd` — `E2E PASSED` (armed through CRSF, climbed, reload re-arms).
6. On a GPU machine: `godot --path godot -s res://tests/shots.gd` and review the screenshots.
7. The exit-code guards: a parse-error test script exits 1; a runtime-error script hangs and is killed by `timeout -k` (rc 124). Both verified on 4.7.2 during planning; re-check if Godot is upgraded.
8. Push and CI are the user's step (the M2a practice): push the branch, check that all five CI jobs go green (`core`, `msrv`, `windows`, `godot`, `sitl`).

The plan was verified against a scratch build of the whole tree before being written (Tasks 1–9 were committed there and every command above was run; Task 10's runner was executed including its failure path; Task 11's mapping is the only piece not executed). If a task's behaviour diverges from what is written here, trust the verified behaviour and fix the plan text.

## Appendix — planning provenance

- **Planning date:** 2026-10-06 (planning resumed and completed in one session after the original planning session hit its usage limit mid-verification).
- **Verified scratch build:** Godot 4.7.2 stable (`Godot_v4.7.2-stable_win64.exe.zip`, SHA-512 checked) with godot-rust 0.5.5 (default API 4.6); the whole change set compiled and tested on Windows (DLL + GDScript suites + both e2e runs) and natively on Linux in WSL (`libofs_godot.so` + workspace tests); `cargo clippy -- -W clippy::incompatible_msrv` clean at msrv 1.85 for the new crates.
- **Versions checked on the day:** Godot stable 4.7.2 (published 2026-08-18); gdext with `compatibility_minimum = 4.6` in the `.gdextension` manifest.
- **Environment notes for live runs:** Windows Firewall blocks freshly built executables talking to Betaflight in WSL — use a scratch `CARGO_TARGET_DIR` or run WSL-native; one SITL per machine; `--test-threads=1` for live Rust tests.
