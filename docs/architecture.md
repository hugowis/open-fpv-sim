# Architecture

How Open FPV Sim fits together, for contributors. The design and its reasons are in
`docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`; the models' equations are in `docs/research/`.

## The shape
A headless server, `ofs-sim`, owns the clock and every model. Clients talk to it over gRPC
(`proto/ofs/v1/sim.proto`): the Python package `ofs` for scripts and tests, and the Godot game through a Rust
extension. Betaflight runs as SITL, a separate process the server launches and steps in lockstep over UDP.

## The crates (`crates/`)
| Crate | What it is | Depends on (ofs-*) |
|---|---|---|
| `ofs-core` | The typed signal bus, the `Model` trait, the scheduler, byte wires, constants and seeded RNG streams. Deterministic, no I/O. | — |
| `ofs-config` | Quad, world and part files (TOML): parsing and validation that reports every problem at once. | core |
| `ofs-physics` | 6-DOF rigid body, propellers, impulse collisions with the world. | core |
| `ofs-electrical` | Battery, ESC and brushless motor. | core |
| `ofs-sensors` | IMU and barometer. | core |
| `ofs-rf` | Shared RF: propagation, Rician fading, receiver diversity, body shadow. | core |
| `ofs-radio` | CRSF framing and the ExpressLRS link. | core, rf |
| `ofs-fc` | The Betaflight SITL bridge (process, UDP exchange, UART taps), MSP, and an open-loop stand-in. | core |
| `ofs-video` | The SmartAudio and Tramp VTX, Betaflight's OSD over MSP DisplayPort, the 5.8 GHz analog link. | core, config, fc, rf |
| `ofs-log` | Recording the bus to MCAP (a model appended last that only reads). | core |
| `ofs-proto` | The gRPC messages and stubs, generated from `proto/ofs/v1/sim.proto`. | — |
| `ofs-sim` | The server: vehicle assembly (`vehicle.rs`), sessions, real-time pacing, the gRPC service. | all of the above |
| `ofs-client` | The Rust client: a worker thread, the FPV composite, the DVR muxer. | core, proto |
| `ofs-godot` | The Godot extension (`OfsClient`), a thin layer over `ofs-client`. | client |

## One tick
The scheduler runs at the quad's base rate (8 kHz for the shipped quads). Each model declares a rate divisor and
runs in a fixed order, reading and writing named signals on the bus (`crates/ofs-core/src/names.rs`). Bytes between
models — the receiver's CRSF into Betaflight's UART, Betaflight's OSD out of its UART — travel on bounded `Wire`s.
The SITL bridge is one model: on its ticks it sends the sensor state and UART bytes in one datagram and waits for
Betaflight's motor outputs, so Betaflight advances exactly as far as the simulation (lockstep). In real-time mode a
pacer runs the same scheduler against the wall clock. Randomness comes from named streams derived from the seed
(`crates/ofs-core/src/rng.rs`), so two runs with the same seed and inputs are bit-identical.

## Clients
- **Python (`python/ofs/`):** `ofs.launch()` starts a server and connects; `Sim` wraps the RPCs (load, run, sticks,
  faults, recording, inputs). `ofs.reader` reads recordings, `ofs.rerun` streams a live view.
- **Godot (`godot/`):** `scripts/app.gd` drives the game; `OfsClient` (from `ofs-godot`) runs the client's worker
  and hands GDScript the pose, telemetry, OSD and the delayed FPV composite; `ui/` holds the OSD, HUD, video and DVR.

## How to add…
- **A part** (a motor, prop, battery…): a TOML in `library/<kind>/` named for its id, with `[provenance]` for every
  parameter (`library/README.md`). A new *kind* also needs its struct in `crates/ofs-config/src/part.rs` and its
  resolution in `crates/ofs-config/src/lib.rs`.
- **A model:** implement `ofs_core::Model` in the crate it belongs to, register its signals' names in
  `crates/ofs-core/src/names.rs`, and add it to the assembly in `crates/ofs-sim/src/vehicle.rs` (the order there
  is the run order). Unit-test it against analytic results in the crate's `tests/`.
- **A fault:** a `Fault` variant and its validation in `crates/ofs-sim/src/vehicle.rs`, the bus signals the model
  reads (`fault.*` in `names.rs`), the message in `proto/ofs/v1/sim.proto` (bump the protocol version) and its
  mapping in `crates/ofs-sim/src/server.rs`, and the Python class in `python/ofs/faults.py`. Document it in
  `docs/research/faults.md`.
- **A recorded signal:** any signal on the bus is recordable by name; its default rate comes from the first matching
  pattern in `ofs_log::DEFAULT_RATES` (`crates/ofs-log/src/spec.rs`).
