# M5: Swappable Radio Components Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split `radio.elrs` into swappable `Handset` / `Link` / `Receiver` parts composed by a `RadioChain`, ship an ELRS 900 MHz band and a geometry-free `fixed` link, and record any real-time flight's sticks to a `play_inputs` CSV that replays deterministically.

**Architecture:** `ofs-radio` gains three traits and a `RadioChain` model; today's `ElrsLink` becomes the first `Link`, the joystick sampling the first `Handset`, the CRSF writer the first `Receiver`. A characterization test, committed first, pins today's bytes so the split provably changes nothing. A recorder at `Vehicle::set_sticks` writes the CSV; protocol 10 adds two RPCs; Python and the Rust/Godot client surface them.

**Tech Stack:** Rust 1.85 workspace (`ofs-radio`, `ofs-config`, `ofs-sim`, `ofs-proto`, `ofs-client`, `ofs-godot`), tonic/prost gRPC, Python client, Godot 4.7.2 GDScript.

**Spec:** `docs/superpowers/specs/2026-10-15-m5-radio-components-design.md` (amended by this plan's rulings, below).

## Global Constraints

- **Byte identity:** `kind = "elrs"` with the 2.4 GHz band must produce the same UART bytes, bus signals and decoded telemetry as before the split. The `elrs` link keeps the RNG stream names `radio.elrs` and `elrs-faults` and its exact draw order. Task 1's goldens are the arbiter; they are never regenerated after Task 1.
- **Only a `Link` draws random numbers.** Handset and receiver are pure.
- **Betaflight keeps seeing CRSF on a UART.** No other FC-side protocol.
- **Rust 1.85 MSRV**, `cargo clippy --workspace --all-targets --locked -- -D warnings` clean, `cargo fmt` clean (CI runs both).
- **Protocol:** `PROTOCOL_VERSION` 9 → 10 in `crates/ofs-proto/src/lib.rs` and `python/ofs/client.py`, in the same task that adds the RPCs.
- **Schema:** `SCHEMA_VERSION` stays 5 (new `[radio]` keys are optional and additive).
- **Recorded files are server-named** under `<data_dir>/logs`; clients never send paths to write.
- **The 900 MHz figures are estimates** and say so (`estimated` provenance wording in the doc comments, as the 2.4 GHz ones do).
- **No new dependencies** in any crate.
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Test commands run from the worktree root. Windows: `cargo test -p <crate> --locked`. Linux (live SITL, Python): WSL, `cd /mnt/c/dev/open-fpv-sim/.worktrees/m5-radio-chain && CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test ...` (a separate target dir per checkout; `cargo clean -p` each workspace crate before switching checkouts that share one).

## Rulings this plan makes against the spec (the spec is amended in Task 9)

1. **Channels between the parts are CRSF 11-bit ticks `[u16; 16]`, not floats** (spec §3 said normalized floats). Quantizing in the handset with `stick_ticks`/`throttle_ticks` is what the monolith does; moving it would risk a one-tick rounding difference and break byte identity. Cost if wrong: a handset with finer resolution than CRSF's cannot express it (YAGNI; Betaflight reads CRSF).
2. **`LinkEnv` is `&Bus`.** A link registers the signal handles it reads (position, attitude, faults) at construction, the repo's pattern for every model. The handset's RF front end (position, antennas, TX power) is read by the builder, which fills `LinkParams`.
3. **`kind` names the link** (`"elrs"` or `"fixed"`); there is no separate `link` key. `handset`, `receiver` and `band` are new optional keys.
4. **`link_stats_interval_packets` moves from `LinkParams` to `ReceiverParams`** (it is the receiver's cadence).
5. **The chain builder is a private function in `ofs-sim/src/vehicle.rs`** beside the antenna/world helpers it uses (not a new file).
6. **The Godot recording toggle is the `I` key** with a HUD indicator (not an F2 control).
7. **Golden digests are files** (`tests/golden/*.digests`) written by `OFS_GOLDEN_WRITE=1` on the pre-split code in Task 1 and only read afterwards.
8. **ELRS 900 MHz:** carrier 915 MHz, LoRa bandwidth 500 kHz (noise floor `-174 + 10 log10(500 000) + 6` dBm), downlink power 100 mW (all estimated); sensitivities from ExpressLRS's published RF-mode table (https://expresslrs.org/info/signal-health/): 25 Hz −123, 50 Hz −120, 100 Hz −117, 200 Hz −112, 250 Hz −111 dBm. `rf_mode` reported in LINK_STATISTICS is the rate's position in the band's ascending rate list plus one (0 stays the 4 Hz Crossfire mode); for 2.4 GHz that reproduces today's 50→1, 150→2, 250→3, 500→4.

## Review Focus

The inputs the spec implies but the happy-path tests would not exercise, each pinned by the named task's tests:

1. A zero-latency link and a one-antenna receiver still reproduce the goldens (Task 1 scenarios `latency-0-250`, `ideal-500`).
2. `packet_rate_hz` invalid for the chosen band (500 with `band = "900"`) is a config error that lists the band's valid rates (Task 3).
3. A key that belongs to another link (`band` with `kind = "fixed"`, `[radio.fixed]` with `kind = "elrs"`) is an error naming the field, not silently ignored (Tasks 3, 4).
4. A recording whose sticks never change after the first row, and a session unloaded mid-recording, both leave a complete, parseable CSV (Task 6).
5. A non-finite or out-of-range stick value never reaches the CSV (it would make the file unplayable) (Task 6).
6. Starting a recording twice, stopping with none running, and recording while the session is unloaded fail with clear `InvalidState`/`NotLoaded` errors, not panics (Task 7).

---

## File Structure

```
crates/ofs-radio/src/
  parts.rs        NEW  traits Handset/Link/Receiver; TxFrame, RfFrontEnd, LinkReport, Delivery; LinkOutputs, FaultSignals, LqWindow
  telemetry.rs    NEW  FcTelemetry, FcTelemetryHandle, apply_frame (moved out of elrs.rs)
  joystick.rs     NEW  JoystickHandset
  receiver.rs     NEW  CrsfReceiver, ReceiverParams, TELEMETRY_FIFO_LEN
  chain.rs        NEW  RadioChain (the Model)
  band.rs         NEW  Band (2g4, 900) tables               (Task 3)
  fixed.rs        NEW  FixedLink, FixedParams              (Task 4)
  elrs.rs         MOD  ElrsLink becomes a Link; constants stay
  lib.rs          MOD  module list
crates/ofs-radio/tests/golden.rs + tests/golden/elrs.digests   NEW  (Task 1)
crates/ofs-sim/src/vehicle.rs        MOD  radio chain builder; InputsRecorder hook (Task 6)
crates/ofs-sim/src/inputs_recorder.rs NEW  InputsRecorder        (Task 6)
crates/ofs-sim/tests/radio_golden.rs + tests/golden/radio-flight.digests   NEW  (Task 1)
crates/ofs-config/src/lib.rs         MOD  RadioSection keys, validation   (Tasks 3-5)
proto/ofs/v1/sim.proto, crates/ofs-proto, python/ofs/*   MOD  protocol 10 (Task 7)
crates/ofs-client, crates/ofs-godot, godot/*             MOD  record toggle (Task 8)
docs/*, README.md, CHANGELOG.md                          MOD/NEW (Task 9)
```

---

### Task 1: Characterization goldens (before any refactor)

**Files:**
- Create: `crates/ofs-radio/tests/golden.rs`
- Create: `crates/ofs-radio/tests/golden/elrs.digests` (generated)
- Create: `crates/ofs-sim/tests/radio_golden.rs`
- Create: `crates/ofs-sim/tests/golden/radio-flight.digests` (generated)

**Interfaces:**
- Consumes: today's `ElrsLink::new(params, div, seed, uart, telemetry_in, &mut bus)`, `ElrsLink::fc_telemetry_handle()`, `LinkParams`, `Vehicle::{start_recording, stop_recording, schedule_fault, set_sticks, run_for}`.
- Produces: two digest files and two tests that every later task must keep green without regenerating the files.

- [ ] **Step 1: Write the radio golden test**

Create `crates/ofs-radio/tests/golden.rs`:

```rust
//! Characterization of the ELRS radio: the bytes it writes to the flight controller's UART, the signals it
//! publishes and the telemetry the handset decodes, hashed per scenario. `tests/golden/elrs.digests` was generated
//! from the pre-split `ElrsLink` (M5 Task 1); the split into handset / link / receiver must reproduce it exactly.
//! Regenerate only on purpose: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-radio --test golden`.
use std::path::PathBuf;

use glam::DVec3;
use ofs_core::shape::Shape;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{crc8_dvb_s2, FRAMETYPE_ATTITUDE, FRAMETYPE_BATTERY_SENSOR, FRAMETYPE_FLIGHT_MODE};
use ofs_radio::elrs::{ElrsLink, FcTelemetryHandle, LinkParams};
use ofs_rf::propagation::{Antenna, AntennaKind, Obstacle, Polarization};

const BASE_HZ: u32 = 8000;
/// 0.4 s: 200 packets at 500 Hz.
const PHASE_TICKS: u64 = 3200;

/// FNV-1a 64 over everything a scenario produces.
struct Digest(u64);

impl Digest {
    fn new() -> Self {
        Digest(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn f64(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
}

fn rig(mut params: LinkParams, stats_interval: u32, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    params.link_stats_interval_packets = stats_interval;
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let div = BASE_HZ / params.packet_rate_hz;
    let link = ElrsLink::new(params, div, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = link.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(link));
    (s, uart, telemetry_in, handle)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

/// One CRC-valid CRSF frame: [address 0xC8][len][type][payload][crc8 poly 0xD5].
fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0xC8, (payload.len() + 2) as u8, frame_type];
    f.extend_from_slice(payload);
    f.push(crc8_dvb_s2(&f[2..]));
    f
}

fn battery_frame(voltage_v: f64, current_a: f64, index: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&((voltage_v * 10.0) as u16).to_be_bytes());
    p.extend_from_slice(&((current_a * 10.0) as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, index]);
    p.push(100);
    frame(FRAMETYPE_BATTERY_SENSOR, &p)
}

fn attitude_frame(pitch_deg: f64, roll_deg: f64, yaw_deg: f64) -> Vec<u8> {
    let rad = |d: f64| (d.to_radians() * 10_000.0).round() as i16;
    let mut p = Vec::new();
    for v in [rad(pitch_deg), rad(roll_deg), rad(yaw_deg)] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    frame(FRAMETYPE_ATTITUDE, &p)
}

/// Six phases of 0.4 s: the quad at 30 m, 3 km, 42 km, 42.8 km, 100 m and 41.5 km (the last three at the
/// sensitivity edge of a 250 mW link, where packets drop), with changing sticks, the link-loss fault in phase 1,
/// the transmitter off in phase 2, a 30 % loss fault in phase 3, a burst fault in phase 5, and telemetry frames
/// offered every phase (twenty batteries in phase 4, to overflow the 16-frame queue).
fn run_scenario(params: LinkParams, stats_interval: u32, seed: u64) -> u64 {
    let (mut s, uart, telemetry_in, fc) = rig(params, stats_interval, seed);
    let mut d = Digest::new();
    let norths = [30.0, 3_000.0, 42_000.0, 42_800.0, 100.0, 41_500.0];
    for (phase, north) in norths.iter().enumerate() {
        let p = phase as f64;
        set_pos(&mut s, DVec3::new(*north, 0.0, -1.7));
        set(&mut s, names::RC_ROLL, 0.1 * p - 0.25);
        set(&mut s, names::RC_PITCH, 0.2 - 0.07 * p);
        set(&mut s, names::RC_YAW, -0.3 + 0.05 * p);
        set(&mut s, names::RC_THROTTLE, 0.15 * p);
        set(&mut s, &names::rc_aux(0), if phase % 2 == 0 { -1.0 } else { 1.0 });
        set(&mut s, names::FAULT_RADIO_LINK_LOSS, if phase == 1 { 1.0 } else { 0.0 });
        set(&mut s, names::RADIO_TX_ENABLED, if phase == 2 { 0.0 } else { 1.0 });
        set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, if phase == 3 { 0.3 } else { 0.0 });
        set(&mut s, names::FAULT_RADIO_BURST_ON_S, if phase == 5 { 0.05 } else { 0.0 });
        set(&mut s, names::FAULT_RADIO_BURST_OFF_S, if phase == 5 { 0.05 } else { 0.0 });
        if phase == 4 {
            for i in 0..20u8 {
                telemetry_in.write(&battery_frame(20.0 + f64::from(i), 1.0, i));
            }
        } else {
            telemetry_in.write(&battery_frame(24.0 + p, p, phase as u8));
            telemetry_in.write(&attitude_frame(p, -p, 10.0 * p));
            telemetry_in.write(&frame(FRAMETYPE_FLIGHT_MODE, b"ACRO\0"));
        }
        for _ in 0..PHASE_TICKS {
            s.step().unwrap();
            d.bytes(&uart.take(usize::MAX));
            for name in [
                names::RADIO_LINK_UP,
                names::RADIO_LQ,
                names::RADIO_RSSI,
                names::RADIO_SNR,
                names::RADIO_ANTENNA,
                names::RADIO_DOWNLINK_LQ,
            ] {
                d.f64(get(&s, name));
            }
        }
    }
    let t = fc.latest();
    for v in [t.battery_v, t.battery_a, t.battery_mah, t.roll_deg, t.pitch_deg, t.yaw_deg, t.last_frame_s] {
        d.f64(v);
    }
    d.bytes(t.flight_mode.as_bytes());
    d.bytes(&t.frame_count.to_le_bytes());
    d.0
}

fn dipole(axis: DVec3) -> Antenna {
    Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Linear, axis }
}

/// (name, params, stats interval in packets, seed). Covers latency 0, latency > 1, one and two antennas, fading
/// and ground bounce, an obstacle, the packet rates that divide the 8 kHz base (50, 250, 500), and a weak link.
fn scenarios() -> Vec<(&'static str, LinkParams, u32, u64)> {
    let faded = |rate: u32| LinkParams { fading: true, ground_bounce: true, ..LinkParams::ideal(rate) };
    let two_antennas = {
        let mut p = faded(500);
        p.quad_antennas.push(Antenna {
            kind: AntennaKind::Patch { beamwidth_deg: 60.0 },
            gain_dbi: 8.0,
            polarization: Polarization::Linear,
            axis: DVec3::NEG_X,
        });
        p
    };
    let obstructed = {
        let mut p = LinkParams { ground_bounce: true, ..LinkParams::ideal(50) };
        let shape = Shape::Box { center: DVec3::new(50.0, 0.0, -10.0), half: DVec3::new(10.0, 10.0, 10.0) };
        p.obstacles = vec![Obstacle { shape: shape.rooted(), rf_loss_db: 25.0 }];
        p
    };
    vec![
        ("ideal-500", LinkParams::ideal(500), 50, 1),
        ("latency-0-250", LinkParams { latency_packets: 0, ..LinkParams::ideal(250) }, 10, 2),
        ("latency-5-500", LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 25, 3),
        ("fading-bounce-500", faded(500), 50, 4),
        ("two-antennas-fading", two_antennas, 20, 5),
        ("obstacle-50hz", obstructed, 5, 6),
        ("weak-10mw", LinkParams { tx_power_mw: 10.0, ..faded(250) }, 50, 7),
        ("crossed-dipole", LinkParams { quad_antennas: vec![dipole(DVec3::Y)], ..faded(500) }, 50, 8),
    ]
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/elrs.digests")
}

#[test]
fn the_elrs_radio_reproduces_its_recorded_digests() {
    let current: Vec<String> = scenarios()
        .into_iter()
        .map(|(name, params, interval, seed)| format!("{name} {:016x}", run_scenario(params, interval, seed)))
        .collect();
    let text = current.join("\n") + "\n";
    if std::env::var_os("OFS_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(golden_path().parent().unwrap()).unwrap();
        std::fs::write(golden_path(), &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(golden_path()).expect("tests/golden/elrs.digests (run once with OFS_GOLDEN_WRITE=1)");
    let expected = expected.replace("\r\n", "\n");
    assert_eq!(text, expected, "the radio's output changed; the split must be byte-identical");
}
```

- [ ] **Step 2: Generate the digests on the unchanged code and check them**

Run: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-radio --test golden --locked` (on Windows PowerShell: `$env:OFS_GOLDEN_WRITE=1; cargo test -p ofs-radio --test golden --locked; Remove-Item Env:OFS_GOLDEN_WRITE`)
Expected: PASS (it writes `crates/ofs-radio/tests/golden/elrs.digests`, eight lines `name 16-hex-digits`).

Run: `cargo test -p ofs-radio --test golden --locked`
Expected: PASS (read mode). Then run it a second time to confirm the digests are stable run to run (same result).

Run: `cargo fmt -p ofs-radio` and `cargo clippy -p ofs-radio --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 3: Write the whole-vehicle golden test**

Create `crates/ofs-sim/tests/radio_golden.rs`:

```rust
//! The radio inside a whole vehicle: two lockstep flights recorded to MCAP, hashed. `tests/golden/radio-flight.digests`
//! was generated from the pre-split code (M5 Task 1); the radio chain must not change a byte of what the vehicle
//! records. Regenerate only on purpose: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden`.
use std::path::{Path, PathBuf};

use ofs_config::{load, FcKind, WorldConfig};
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks};

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// A 2 s open-loop flight (throttle and roll steps, a 30 % radio-loss fault from 1 s) recorded with the default
/// table; returns the digest of the MCAP file.
fn flight(quad: &str, seed: u64, path: &Path) -> u64 {
    let quad_path = format!("{}/../../quads/{quad}", env!("CARGO_MANIFEST_DIR"));
    let cfg = load(Path::new(&quad_path)).unwrap();
    let opts = BuildOptions { seed, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    let mut v = build(&cfg, &opts).unwrap();
    v.start_recording(&[], path).unwrap();
    v.schedule_fault(Fault::RadioLoss { fraction: 0.3 }, 1.0).unwrap();
    for (throttle, roll, secs) in [(0.0, 0.0, 0.5), (0.6, 0.0, 0.75), (0.4, 0.5, 0.75)] {
        v.set_sticks(&Sticks { throttle, roll, ..Sticks::default() });
        v.run_for(secs).unwrap();
    }
    v.stop_recording().unwrap();
    fnv1a(&std::fs::read(path).unwrap())
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/radio-flight.digests")
}

#[test]
fn a_recorded_vehicle_flight_reproduces_its_digests() {
    let dir = tempfile::tempdir().unwrap();
    let current = [
        ("freestyle", flight("opendrone-5f-freestyle.toml", 1, &dir.path().join("a.mcap"))),
        ("weak-link", flight("opendrone-5f-weak-link.toml", 3, &dir.path().join("b.mcap"))),
    ];
    let text: String = current.iter().map(|(name, d)| format!("{name} {d:016x}\n")).collect();
    if std::env::var_os("OFS_GOLDEN_WRITE").is_some() {
        std::fs::create_dir_all(golden_path().parent().unwrap()).unwrap();
        std::fs::write(golden_path(), &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(golden_path()).expect("tests/golden/radio-flight.digests (run once with OFS_GOLDEN_WRITE=1)");
    assert_eq!(text, expected.replace("\r\n", "\n"), "the vehicle's recording changed; the radio chain must be byte-identical");
}
```

If `tempfile` is not a dev-dependency of `ofs-sim`, it is: `tests/recording.rs` uses it.

- [ ] **Step 4: Generate and check the vehicle digests**

Run: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden --locked` (PowerShell form as above), then `cargo test -p ofs-sim --test radio_golden --locked`
Expected: PASS twice (the second is read mode).

- [ ] **Step 5: Normalize line endings and commit**

The digest files are LF (the repo normalizes to LF; the tests also tolerate CRLF).

```bash
git add crates/ofs-radio/tests/golden.rs crates/ofs-radio/tests/golden crates/ofs-sim/tests/radio_golden.rs crates/ofs-sim/tests/golden
git commit -m "test(radio): goldens for the ELRS radio, before the split

Hashes the UART bytes, published signals and decoded telemetry of eight scenarios, and the MCAP of two
whole-vehicle flights. M5's refactor must reproduce them exactly.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---
### Task 2: Split the radio into Handset / Link / Receiver and wire the vehicle

**Files:**
- Create: `crates/ofs-radio/src/parts.rs`, `telemetry.rs`, `joystick.rs`, `receiver.rs`, `chain.rs`
- Modify: `crates/ofs-radio/src/elrs.rs` (ElrsLink becomes a `Link`), `crates/ofs-radio/src/lib.rs`
- Modify: `crates/ofs-radio/tests/elrs.rs`, `crates/ofs-radio/tests/golden.rs` (the `rig` helper only)
- Create: `crates/ofs-radio/tests/parts.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs` (radio construction, imports)

**Interfaces:**
- Consumes: Task 1's goldens (must stay green, unregenerated); `crsf::{rc_channels_frame, link_statistics_frame, stick_ticks, throttle_ticks, Decoder, Frame, LinkStatistics, CHANNEL_COUNT}`; `ofs_core::{Bus, Model, Signal, StepCtx, Wire}`.
- Produces (later tasks rely on these exact names):
  - `ofs_radio::parts::{Channels, TxFrame, RfFrontEnd, Handset, LinkReport, Delivery, Link, Receiver, LinkOutputs, FaultSignals, LqWindow, LQ_WINDOW}`
  - `ofs_radio::telemetry::{FcTelemetry, FcTelemetryHandle, apply_frame}`
  - `ofs_radio::joystick::JoystickHandset::new(front_end: RfFrontEnd, bus: &mut Bus) -> Self`
  - `ofs_radio::receiver::{CrsfReceiver, ReceiverParams, TELEMETRY_FIFO_LEN}`; `CrsfReceiver::new(params: ReceiverParams, uart: Wire, telemetry_in: Wire) -> Self`
  - `ofs_radio::elrs::ElrsLink::new(params: LinkParams, seed: u64, bus: &mut Bus) -> Self` (no uart, no divisor)
  - `ofs_radio::chain::RadioChain::{new(handset, link, receiver, base_hz, bus), elrs(params, receiver_params, base_hz, seed, uart, telemetry_in, bus), fc_telemetry_handle()}`
  - `LinkParams` loses `link_stats_interval_packets`.

- [ ] **Step 1: Write the failing part tests**

Create `crates/ofs-radio/tests/parts.rs`:

```rust
use glam::DVec3;
use ofs_core::{names, Bus, Wire};
use ofs_radio::crsf::{self, Decoder, Frame};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::{Delivery, Handset, LinkReport, LqWindow, Receiver, RfFrontEnd};
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};

fn front_end() -> RfFrontEnd {
    RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 }
}

fn report() -> LinkReport {
    LinkReport {
        link_up: true,
        lq_pct: 100.0,
        rssi_dbm: -50.0,
        snr_db: 40.0,
        antenna: 0,
        downlink_lq_pct: 100.0,
        uplink_rssi_dbm: [-50.0, -60.0],
        uplink_snr_db: 40.0,
        downlink_rssi_dbm: -55.0,
        downlink_snr_db: 35.0,
        rf_mode: 4,
        tx_power_index: 17,
    }
}

fn delivery(channels: Option<[u16; 16]>) -> Delivery {
    Delivery { channels, downlink_slot: false, report: report() }
}

#[test]
fn the_lq_window_is_the_share_received_of_the_last_hundred() {
    let mut w = LqWindow::new();
    assert_eq!(w.push(false), 0.0);
    let mut lq = 0.0;
    for _ in 0..99 {
        lq = w.push(true);
    }
    assert_eq!(lq, 99.0, "99 of the last 100 (the first miss is still inside)");
    assert_eq!(w.push(true), 100.0, "the miss has left the window");
}

#[test]
fn the_joystick_handset_orders_channels_aetr_and_follows_the_transmitter_signal() {
    let mut bus = Bus::new();
    let mut handset = JoystickHandset::new(front_end(), &mut bus);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_THROTTLE, 0.25), (names::RC_YAW, 0.5)] {
        let s = bus.signal::<f64>(name);
        bus.set(s, v);
    }
    let aux1 = bus.signal::<f64>(&names::rc_aux(0));
    bus.set(aux1, 1.0);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let frame = handset.sample(&bus);
    assert!(frame.transmitting);
    assert_eq!(
        (frame.channels[0], frame.channels[1], frame.channels[2], frame.channels[3], frame.channels[4]),
        (crsf::stick_ticks(0.2), crsf::stick_ticks(-0.2), crsf::throttle_ticks(0.25), crsf::stick_ticks(0.5), crsf::stick_ticks(1.0)),
        "roll, pitch, throttle, yaw (AETR), then AUX1"
    );
    assert_eq!(frame.channels[15], crsf::stick_ticks(0.0), "the unused channels sit centred");
    bus.set(tx, 0.0);
    assert!(!handset.sample(&bus).transmitting);
}

#[test]
fn the_crsf_receiver_writes_a_frame_per_delivered_packet_and_statistics_every_n() {
    let (uart, tap) = (Wire::new(4096), Wire::new(4096));
    let mut rx = CrsfReceiver::new(ReceiverParams { link_stats_interval_packets: 3 }, uart.clone(), tap);
    let channels = [crsf::stick_ticks(0.1); 16];
    for _ in 0..6 {
        rx.emit(&delivery(Some(channels)));
    }
    rx.emit(&delivery(None));
    let frames = Decoder::default().push(&uart.take(usize::MAX));
    let rc = frames.iter().filter(|f| matches!(f, Frame::RcChannels(_))).count();
    let stats: Vec<_> = frames.iter().filter_map(|f| if let Frame::LinkStatistics(s) = f { Some(*s) } else { None }).collect();
    assert_eq!(rc, 6, "none for the lost packet");
    assert_eq!(stats.len(), 2, "after the 3rd and the 6th delivered packet");
    assert_eq!((stats[0].uplink_rssi_1, stats[0].uplink_rssi_2, stats[0].rf_mode, stats[0].uplink_tx_power), (50, 60, 4, 17));
}

#[test]
fn the_receiver_queues_flight_controller_frames_and_drops_the_oldest_past_sixteen() {
    let (uart, tap) = (Wire::new(4096), Wire::new(1 << 16));
    let mut rx = CrsfReceiver::new(ReceiverParams::default(), uart, tap.clone());
    for i in 0..20u8 {
        let mut f = vec![0xC8, 3, 0x08, i];
        f.push(crsf::crc8_dvb_s2(&f[2..]));
        tap.write(&f);
    }
    rx.ingest_telemetry();
    let first = rx.next_telemetry().unwrap();
    assert_eq!((first.0, first.1), (0x08, vec![4]), "frames 0..3 were dropped: the queue holds the newest 16");
    let mut left = 1;
    while rx.next_telemetry().is_some() {
        left += 1;
    }
    assert_eq!(left, 16);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p ofs-radio --test parts --locked`
Expected: FAIL to compile (`unresolved import ofs_radio::joystick` and friends).

- [ ] **Step 3: Create `parts.rs`**

```rust
//! The radio's three swappable parts and what flows between them.
//!
//! One packet slot flows `Handset -> Link -> Receiver`: the handset samples the sticks, the link decides what
//! the air does to the packet and what comes back, the receiver turns what arrived into the bytes the flight
//! controller reads. Only a [`Link`] may draw random numbers: handsets and receivers are pure, so a seeded run
//! never depends on which parts are swapped in around a link.
use std::collections::VecDeque;

use glam::DVec3;
use ofs_core::{names, Bus, Signal, StepCtx};
use ofs_rf::propagation::Antenna;

use crate::crsf::CHANNEL_COUNT;

/// The 16 CRSF channels as 11-bit ticks (the currency between the parts: Betaflight reads CRSF).
pub type Channels = [u16; CHANNEL_COUNT];

/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;

/// What the handset sent in one packet slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TxFrame {
    pub channels: Channels,
    /// The transmitter is switched on (the pilot is connected, the radio is not cut).
    pub transmitting: bool,
}

/// The handset's radio end: where it is, its antennas (axes already in the world frame) and its TX power.
#[derive(Debug, Clone)]
pub struct RfFrontEnd {
    pub position: DVec3,
    pub antennas: Vec<Antenna>,
    pub tx_power_mw: f64,
}

/// The controller: stick and switch signals in, one [`TxFrame`] per packet slot out.
pub trait Handset: Send {
    /// Where it transmits from; the chain builder uses it to fill a link's transmitter end.
    fn rf_front_end(&self) -> &RfFrontEnd;
    fn sample(&mut self, bus: &Bus) -> TxFrame;
}

/// What a link tells the rest of the sim about a packet slot: the six bus outputs, and what a receiver reports
/// in LINK_STATISTICS.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkReport {
    // Published on the bus (the HUD, the state stream and the recorder read these).
    pub link_up: bool,
    pub lq_pct: f64,
    /// The active antenna's RSSI; the no-signal floor while the link is down.
    pub rssi_dbm: f64,
    pub snr_db: f64,
    pub antenna: usize,
    pub downlink_lq_pct: f64,
    // What the receiver reports in LINK_STATISTICS (raw, whether or not the link is up).
    /// Per receiver antenna; with one antenna both entries hold its RSSI.
    pub uplink_rssi_dbm: [f64; 2],
    pub uplink_snr_db: f64,
    pub downlink_rssi_dbm: f64,
    pub downlink_snr_db: f64,
    pub rf_mode: u8,
    pub tx_power_index: u8,
}

/// One packet slot's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    /// The channels the receiver heard (after the link's latency); `None` when the packet was lost.
    pub channels: Option<Channels>,
    /// A downlink telemetry slot flew and the handset heard it: one queued frame reaches the handset.
    pub downlink_slot: bool,
    pub report: LinkReport,
}

/// The air between handset and receiver. Implementations register the bus signals they read (the quad's pose,
/// the radio faults) at construction.
pub trait Link: Send {
    /// The model's name: it names the chain and seeds the link's random stream.
    fn name(&self) -> &str;
    fn packet_rate_hz(&self) -> u32;
    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery;
}

/// The receiver on the quad: what arrives becomes CRSF bytes on the flight controller's UART.
pub trait Receiver: Send {
    /// Reads what the flight controller sent on its UART TX and queues the telemetry frames for downlink slots.
    fn ingest_telemetry(&mut self);
    /// Writes what the packet slot delivered to the flight controller.
    fn emit(&mut self, delivery: &Delivery);
    /// The next queued telemetry frame (type, payload), when a downlink slot flies.
    fn next_telemetry(&mut self) -> Option<(u8, Vec<u8>)>;
}

/// The six signals every link publishes.
pub struct LinkOutputs {
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    snr: Signal<f64>,
    antenna: Signal<f64>,
    downlink_lq: Signal<f64>,
}

impl LinkOutputs {
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            snr: bus.signal(names::RADIO_SNR),
            antenna: bus.signal(names::RADIO_ANTENNA),
            downlink_lq: bus.signal(names::RADIO_DOWNLINK_LQ),
        }
    }

    pub fn publish(&self, bus: &mut Bus, report: &LinkReport) {
        bus.set(self.link_up, if report.link_up { 1.0 } else { 0.0 });
        bus.set(self.lq, report.lq_pct);
        bus.set(self.rssi, report.rssi_dbm);
        bus.set(self.snr, report.snr_db);
        bus.set(self.antenna, report.antenna as f64);
        bus.set(self.downlink_lq, report.downlink_lq_pct);
    }
}

/// The radio faults every link honours (the fault catalog is link-agnostic).
pub struct FaultSignals {
    link_loss: Signal<f64>,
    loss_fraction: Signal<f64>,
    burst_on: Signal<f64>,
    burst_off: Signal<f64>,
}

impl FaultSignals {
    pub fn register(bus: &mut Bus) -> Self {
        Self {
            link_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            loss_fraction: bus.signal(names::FAULT_RADIO_LOSS_FRACTION),
            burst_on: bus.signal(names::FAULT_RADIO_BURST_ON_S),
            burst_off: bus.signal(names::FAULT_RADIO_BURST_OFF_S),
        }
    }

    /// The link-loss fault is active: no packet reaches anyone. A NaN signal counts as cut (it fails safe, as the
    /// pre-split `< 0.5` test did).
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn link_cut(&self, bus: &Bus) -> bool {
        !(bus.get(self.link_loss) < 0.5)
    }

    /// The packet survives the extra-loss and burst faults. `fault_draw` is the link's own per-packet draw from its
    /// fault stream (drawn every packet, fault or not, so the link's other draws never shift with a fault).
    pub fn survives(&self, bus: &Bus, time_s: f64, fault_draw: f64) -> bool {
        let fraction = bus.get(self.loss_fraction);
        let burst_on = bus.get(self.burst_on);
        let burst_off = bus.get(self.burst_off);
        let in_burst = burst_on > 0.0 && time_s % (burst_on + burst_off) < burst_on;
        fault_draw >= fraction && !in_burst
    }
}

/// Link quality: the share received of the last [`LQ_WINDOW`] packets, in percent.
#[derive(Debug)]
pub struct LqWindow(VecDeque<bool>);

impl LqWindow {
    pub fn new() -> Self {
        Self(VecDeque::with_capacity(LQ_WINDOW + 1))
    }

    /// Records one packet and returns the link quality including it.
    pub fn push(&mut self, received: bool) -> f64 {
        self.0.push_back(received);
        if self.0.len() > LQ_WINDOW {
            self.0.pop_front();
        }
        100.0 * self.0.iter().filter(|r| **r).count() as f64 / self.0.len() as f64
    }
}

impl Default for LqWindow {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Create `telemetry.rs`**

Move `FcTelemetry`, `FcTelemetryHandle` and `apply_telemetry_frame` out of `elrs.rs` (delete them there) into `crates/ofs-radio/src/telemetry.rs`:

```rust
//! What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink slot.
use std::sync::{Arc, Mutex};

use crate::crsf;

/// What Betaflight told the handset, decoded from the CRSF telemetry frames that flew a downlink
/// slot. Units are Betaflight's own CRSF encodings (see docs/research/elrs-link.md): the battery
/// voltage in 0.1 V and current in 0.1 A (`getLegacyBatteryVoltage()`/`getAmperage()/10`), attitude
/// in radians x 10 000 with yaw wrapped to ±180°, the flight mode verbatim.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FcTelemetry {
    pub battery_v: f64,
    pub battery_a: f64,
    pub battery_mah: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    pub yaw_deg: f64,
    pub flight_mode: String,
    /// Simulated time of the last frame that flew a slot (0 until one arrives).
    pub last_frame_s: f64,
    /// Frames that flew a slot so far (a monotonic "anything arrived yet" marker).
    pub frame_count: u64,
}

/// The latest [`FcTelemetry`], shared with the server's state snapshot (the `OsdHandle` pattern).
#[derive(Debug, Clone, Default)]
pub struct FcTelemetryHandle(Arc<Mutex<FcTelemetry>>);

impl FcTelemetryHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn latest(&self) -> FcTelemetry {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn store(&self, t: FcTelemetry) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = t;
    }
}

/// Decodes one telemetry frame that flew a downlink slot into the shared handle. Frame types we do not expose
/// still consume their slot (the radio forwarded them; nothing more is modelled).
pub fn apply_frame(fc: &FcTelemetryHandle, frame_type: u8, payload: &[u8], now_s: f64) {
    let mut t = fc.latest();
    match frame_type {
        crsf::FRAMETYPE_BATTERY_SENSOR if payload.len() >= 7 => {
            t.battery_v = f64::from(u16::from_be_bytes([payload[0], payload[1]])) / 10.0;
            t.battery_a = f64::from(u16::from_be_bytes([payload[2], payload[3]])) / 10.0;
            t.battery_mah = f64::from(u32::from_be_bytes([0, payload[4], payload[5], payload[6]]));
        }
        crsf::FRAMETYPE_ATTITUDE if payload.len() >= 6 => {
            let deg = |raw: [u8; 2]| f64::from(i16::from_be_bytes(raw)) * 180.0 / (10_000.0 * std::f64::consts::PI);
            t.pitch_deg = deg([payload[0], payload[1]]);
            t.roll_deg = deg([payload[2], payload[3]]);
            t.yaw_deg = deg([payload[4], payload[5]]);
        }
        crsf::FRAMETYPE_FLIGHT_MODE => {
            t.flight_mode = String::from_utf8_lossy(payload).split('\0').next().unwrap_or("").to_owned();
        }
        _ => {}
    }
    t.frame_count += 1;
    t.last_frame_s = now_s;
    fc.store(t);
}
```

- [ ] **Step 5: Create `joystick.rs`**

```rust
//! The first handset: the stick and switch signals the joystick, the Pilot stream or `SetSticks` drive.
use ofs_core::{names, Bus, Signal};

use crate::crsf::{self, CHANNEL_COUNT};
use crate::parts::{Handset, RfFrontEnd, TxFrame};

pub struct JoystickHandset {
    front_end: RfFrontEnd,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
}

impl JoystickHandset {
    pub fn new(front_end: RfFrontEnd, bus: &mut Bus) -> Self {
        Self {
            front_end,
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
        }
    }
}

impl Handset for JoystickHandset {
    fn rf_front_end(&self) -> &RfFrontEnd {
        &self.front_end
    }

    /// Sticks to channels in Betaflight's default AETR order, then AUX1..4, the rest centred.
    fn sample(&mut self, bus: &Bus) -> TxFrame {
        let mut ch = [crsf::stick_ticks(0.0); CHANNEL_COUNT];
        ch[0] = crsf::stick_ticks(bus.get(self.roll));
        ch[1] = crsf::stick_ticks(bus.get(self.pitch));
        ch[2] = crsf::throttle_ticks(bus.get(self.throttle));
        ch[3] = crsf::stick_ticks(bus.get(self.yaw));
        for (slot, s) in ch[4..].iter_mut().zip(&self.aux) {
            *slot = crsf::stick_ticks(bus.get(*s));
        }
        TxFrame { channels: ch, transmitting: bus.get(self.tx_enabled) > 0.5 }
    }
}
```

- [ ] **Step 6: Create `receiver.rs`**

```rust
//! The first receiver: one CRSF RC frame per delivered packet, LINK_STATISTICS every N delivered packets, and the
//! flight controller's own CRSF telemetry queued for the downlink slots.
use std::collections::VecDeque;

use ofs_core::Wire;

use crate::crsf::{self, Decoder, Frame, LinkStatistics};
use crate::parts::{Delivery, Receiver};

/// How many offered telemetry frames wait for a slot at most; the oldest is dropped when full (stale data yields
/// to fresh, like a radio that cannot retransmit).
pub const TELEMETRY_FIFO_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverParams {
    /// A LINK_STATISTICS frame follows every this many delivered packets.
    pub link_stats_interval_packets: u32,
}

impl Default for ReceiverParams {
    fn default() -> Self {
        Self { link_stats_interval_packets: 50 }
    }
}

pub struct CrsfReceiver {
    params: ReceiverParams,
    /// The flight controller's UART RX: the receiver writes here.
    uart: Wire,
    /// The flight controller's UART TX: its CRSF telemetry arrives here.
    telemetry_in: Wire,
    decoder: Decoder,
    fifo: VecDeque<(u8, Vec<u8>)>,
    since_stats: u32,
}

impl CrsfReceiver {
    pub fn new(params: ReceiverParams, uart: Wire, telemetry_in: Wire) -> Self {
        Self { params, uart, telemetry_in, decoder: Decoder::default(), fifo: VecDeque::new(), since_stats: 0 }
    }
}

impl Receiver for CrsfReceiver {
    fn ingest_telemetry(&mut self) {
        // Whatever Betaflight wrote this tick joins the FIFO (oldest dropped when full). No random draws: the
        // queue is a pure function of the frames offered and the slots flown.
        for frame in self.decoder.push(&self.telemetry_in.take(usize::MAX)) {
            if let Frame::Other { frame_type, payload, .. } = frame {
                if self.fifo.len() >= TELEMETRY_FIFO_LEN {
                    self.fifo.pop_front();
                }
                self.fifo.push_back((frame_type, payload));
            }
        }
    }

    fn emit(&mut self, delivery: &Delivery) {
        let Some(channels) = delivery.channels else { return };
        self.uart.write(&crsf::rc_channels_frame(&channels));
        self.since_stats += 1;
        if self.since_stats >= self.params.link_stats_interval_packets {
            self.since_stats = 0;
            let r = &delivery.report;
            let dbm_field = |dbm: f64| (-dbm).round().clamp(0.0, 255.0) as u8;
            self.uart.write(&crsf::link_statistics_frame(&LinkStatistics {
                uplink_rssi_1: dbm_field(r.uplink_rssi_dbm[0]),
                uplink_rssi_2: dbm_field(r.uplink_rssi_dbm[1]),
                uplink_lq: r.lq_pct.round().clamp(0.0, 100.0) as u8,
                uplink_snr: r.uplink_snr_db.round().clamp(-128.0, 127.0) as i8,
                active_antenna: r.antenna as u8,
                rf_mode: r.rf_mode,
                uplink_tx_power: r.tx_power_index,
                downlink_rssi: dbm_field(r.downlink_rssi_dbm),
                downlink_lq: r.downlink_lq_pct.round().clamp(0.0, 100.0) as u8,
                downlink_snr: r.downlink_snr_db.round().clamp(-128.0, 127.0) as i8,
            }));
        }
    }

    fn next_telemetry(&mut self) -> Option<(u8, Vec<u8>)> {
        self.fifo.pop_front()
    }
}
```

- [ ] **Step 7: Rewrite `elrs.rs` so `ElrsLink` is a `Link`**

Keep, unchanged, everything from the top of the file through `LinkParams::ideal` (the constants, `sensitivity_dbm`, `rf_mode_index`, `tx_power_index_mw`, `packet_error_rate`, `LinkParams`), with these edits: delete `LQ_WINDOW` (now in `parts`), delete `TELEMETRY_FIFO_LEN` (now in `receiver`), delete the `link_stats_interval_packets` field and its doc line from `LinkParams` and from `ideal()`, delete the `FcTelemetry`, `FcTelemetryHandle` items (now in `telemetry`). Replace the imports and everything from `struct Inputs` to the end of the file with:

```rust
use std::collections::VecDeque;

use glam::{DQuat, DVec3};
use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Signal, StepCtx};
use ofs_rf::fading::{body_shadow_db, Diversity, Fader, LOS_K_DB};
use ofs_rf::propagation::{mw_to_dbm, path_gain, wavelength_m, Antenna, AntennaKind, Endpoint, Obstacle, Polarization};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::parts::{Channels, Delivery, FaultSignals, Link, LinkReport, LqWindow, TxFrame};

/// The ExpressLRS link: the RF budget of both directions on the shared propagation model, a logistic packet-loss
/// curve around the mode's sensitivity, the handset-to-receiver latency pipeline, link quality, and the downlink
/// telemetry slots. Deterministic: every packet draws the same count of numbers from the model's own seeded
/// stream, whatever the inputs.
pub struct ElrsLink {
    params: LinkParams,
    rng: ChaCha8Rng,
    fault_rng: ChaCha8Rng,
    faults: FaultSignals,
    pos: Signal<DVec3>,
    att: Signal<DQuat>,
    history: LqWindow,
    downlink_history: LqWindow,
    pipeline: VecDeque<Channels>,
    quad_diversity: Diversity,
    handset_diversity: Diversity,
    quad_faders: Vec<Fader>,
    handset_faders: Vec<Fader>,
    /// Reused per-packet scratch: the per-antenna RSSI vectors (no allocation in the hot loop).
    rssi: Vec<f64>,
    downlink: Vec<f64>,
    last_pos: Option<DVec3>,
    downlink_packets: u32,
}

impl ElrsLink {
    pub fn new(params: LinkParams, seed: u64, bus: &mut Bus) -> Self {
        assert!(!params.quad_antennas.is_empty() && !params.handset_antennas.is_empty(), "both ends need at least one antenna");
        assert!(sensitivity_dbm(params.packet_rate_hz).is_some(), "packet_rate_hz must be one of 50, 150, 250, 500");
        let mut rng = model_rng(seed, MODEL_NAME);
        let fault_rng = model_rng(seed, "elrs-faults");
        let mut quad_faders = vec![Fader::default(); params.quad_antennas.len()];
        let mut handset_faders = vec![Fader::default(); params.handset_antennas.len()];
        for f in quad_faders.iter_mut().chain(handset_faders.iter_mut()) {
            f.next(&mut rng, 0.0, LOS_K_DB); // a fresh scatter state for the first packet
        }
        let (quad_antennas, handset_antennas) = (params.quad_antennas.len(), params.handset_antennas.len());
        Self {
            params,
            rng,
            fault_rng,
            faults: FaultSignals::register(bus),
            pos: bus.signal(names::BODY_POS_NED),
            att: bus.signal(names::BODY_ATT),
            history: LqWindow::new(),
            downlink_history: LqWindow::new(),
            pipeline: VecDeque::new(),
            quad_diversity: Diversity::default(),
            handset_diversity: Diversity::default(),
            quad_faders,
            handset_faders,
            rssi: vec![NO_SIGNAL_RSSI_DBM; quad_antennas],
            downlink: vec![NO_SIGNAL_RSSI_DBM; handset_antennas],
            last_pos: None,
            downlink_packets: 0,
        }
    }
}

impl Link for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn packet_rate_hz(&self) -> u32 {
        self.params.packet_rate_hz
    }

    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery {
        // The pipeline delays samples by `latency_packets` packets.
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(tx.channels, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(tx.channels);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        let pos = bus.get(self.pos);
        let att = bus.get(self.att);
        let att = if att.length_squared() > 0.0 { att.normalize() } else { DQuat::IDENTITY };
        let moved = self.last_pos.map_or(0.0, |p| p.distance(pos));
        self.last_pos = Some(pos);
        let sensitivity = sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction");
        let transmitting = tx.transmitting && !self.faults.link_cut(bus);

        // Draw order is fixed: every packet draws the same count of numbers whatever the inputs — the uplink
        // fades (two normals per quad antenna), the uplink loss draw, the downlink fades, the downlink draw.
        let rho = (-moved / (wavelength_m(ELRS_FREQ_MHZ) * 0.5)).exp();
        let shadow = body_shadow_db(att, pos, self.params.handset_position);
        // The handset transmits on its active antenna: invariant across the quad's antennas.
        let tx_ant = self.params.handset_antennas[self.handset_diversity.active().min(self.params.handset_antennas.len() - 1)];
        self.rssi.clear();
        self.rssi.resize(self.params.quad_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let rssi = &mut self.rssi;
        for (j, configured) in self.params.quad_antennas.iter().enumerate() {
            let rx_ant = Antenna { axis: (att * configured.axis).normalize(), ..*configured };
            let path = path_gain(
                &Endpoint { position: self.params.handset_position, antenna: tx_ant },
                &Endpoint { position: pos, antenna: rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.quad_faders[j].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            rssi[j] = (mw_to_dbm(self.params.tx_power_mw) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let active = self.quad_diversity.choose(rssi);
        let draw: f64 = self.rng.gen();
        // Drawn every packet from its own stream, fault or not, so the model's own draws (fading, loss, downlink)
        // never shift with a fault (tests/elrs.rs: the_models_own_stream_ignores_the_fault_fraction).
        let fault_draw: f64 = self.fault_rng.gen();
        let received = transmitting
            && draw >= packet_error_rate(rssi[active], sensitivity)
            && self.faults.survives(bus, ctx.time_s, fault_draw);

        // Downlink: the reversed path of the active antenna, the receiver transmitting at DOWNLINK_TX_POWER_MW,
        // received on the handset's best antenna.
        let configured = self.params.quad_antennas[active];
        let tx_ant = Antenna { axis: (att * configured.axis).normalize(), ..configured };
        self.downlink.clear();
        self.downlink.resize(self.params.handset_antennas.len(), NO_SIGNAL_RSSI_DBM);
        let downlink = &mut self.downlink;
        for (a, rx_ant) in self.params.handset_antennas.iter().enumerate() {
            let path = path_gain(
                &Endpoint { position: pos, antenna: tx_ant },
                &Endpoint { position: self.params.handset_position, antenna: *rx_ant },
                ELRS_FREQ_MHZ,
                &self.params.obstacles,
                self.params.ground_bounce,
            );
            let fade = if self.params.fading {
                self.handset_faders[a].next(&mut self.rng, rho, LOS_K_DB - path.obstruction_db)
            } else {
                0.0
            };
            downlink[a] = (mw_to_dbm(DOWNLINK_TX_POWER_MW) + path.gain_db + fade - shadow).max(NO_SIGNAL_RSSI_DBM);
        }
        let downlink_active = self.handset_diversity.choose(downlink);
        let downlink_draw: f64 = self.rng.gen();
        let downlink_received = transmitting && downlink_draw >= packet_error_rate(downlink[downlink_active], sensitivity);

        // One downlink packet in TELEMETRY_SLOT_DIVISOR carries a telemetry frame, if it is heard.
        let mut downlink_slot = false;
        if transmitting {
            self.downlink_packets += 1;
            downlink_slot = downlink_received && self.downlink_packets % TELEMETRY_SLOT_DIVISOR == 0;
        }

        // Link quality over the last 100 packets, per direction; silence on loss; Betaflight fails safe.
        let lq = self.history.push(received);
        let downlink_lq = self.downlink_history.push(downlink_received);
        let link_up = lq > 0.0;
        let snr = rssi[active] - NOISE_FLOOR_DBM;

        Delivery {
            channels: received.then_some(channels),
            downlink_slot,
            report: LinkReport {
                link_up,
                lq_pct: lq,
                rssi_dbm: if link_up { rssi[active] } else { NO_SIGNAL_RSSI_DBM },
                snr_db: if link_up { snr } else { NO_SIGNAL_RSSI_DBM - NOISE_FLOOR_DBM },
                antenna: active,
                downlink_lq_pct: downlink_lq,
                uplink_rssi_dbm: [rssi[0], rssi.get(1).copied().unwrap_or(rssi[0])],
                uplink_snr_db: snr,
                downlink_rssi_dbm: downlink[downlink_active],
                downlink_snr_db: downlink[downlink_active] - NOISE_FLOOR_DBM,
                rf_mode: rf_mode_index(self.params.packet_rate_hz).unwrap_or(0),
                tx_power_index: tx_power_index_mw(self.params.tx_power_mw as u32),
            },
        }
    }
}
```

The `use` block at the top of the file is replaced by the one above; keep the module's top doc comment. The `Inputs`/`Outputs` structs, the old `ElrsLink` impl, `apply_telemetry_frame`, and `impl Model for ElrsLink` are gone.

- [ ] **Step 8: Create `chain.rs` and update `lib.rs`**

`crates/ofs-radio/src/chain.rs`:

```rust
//! The radio as one model: a handset, a link and a receiver stepped in order every packet slot.
use ofs_core::{Bus, Model, SimError, StepCtx, Wire};

use crate::elrs::{ElrsLink, LinkParams};
use crate::joystick::JoystickHandset;
use crate::parts::{Handset, Link, LinkOutputs, Receiver, RfFrontEnd};
use crate::receiver::{CrsfReceiver, ReceiverParams};
use crate::telemetry::{self, FcTelemetryHandle};

pub struct RadioChain {
    handset: Box<dyn Handset>,
    link: Box<dyn Link>,
    receiver: Box<dyn Receiver>,
    outputs: LinkOutputs,
    fc_telemetry: FcTelemetryHandle,
    div: u32,
}

impl RadioChain {
    /// The chain runs at the link's packet rate, which must divide the base rate.
    pub fn new(handset: Box<dyn Handset>, link: Box<dyn Link>, receiver: Box<dyn Receiver>, base_hz: u32, bus: &mut Bus) -> Self {
        let rate = link.packet_rate_hz();
        assert!(rate > 0 && base_hz % rate == 0, "the link's packet rate ({rate} Hz) must divide the base rate ({base_hz} Hz)");
        Self { handset, link, receiver, outputs: LinkOutputs::register(bus), fc_telemetry: FcTelemetryHandle::new(), div: base_hz / rate }
    }

    /// The ELRS 2.4 GHz chain with the joystick handset and the CRSF receiver: what `kind = "elrs"` builds. The
    /// handset's radio end is read from `params`.
    pub fn elrs(params: LinkParams, receiver: ReceiverParams, base_hz: u32, seed: u64, uart: Wire, telemetry_in: Wire, bus: &mut Bus) -> Self {
        let front_end = RfFrontEnd {
            position: params.handset_position,
            antennas: params.handset_antennas.clone(),
            tx_power_mw: params.tx_power_mw,
        };
        let handset = JoystickHandset::new(front_end, bus);
        let link = ElrsLink::new(params, seed, bus);
        let receiver = CrsfReceiver::new(receiver, uart, telemetry_in);
        Self::new(Box::new(handset), Box::new(link), Box::new(receiver), base_hz, bus)
    }

    /// The handset's view of Betaflight's telemetry, updated as frames fly downlink slots.
    pub fn fc_telemetry_handle(&self) -> FcTelemetryHandle {
        self.fc_telemetry.clone()
    }
}

impl Model for RadioChain {
    fn name(&self) -> &str {
        self.link.name()
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let tx = self.handset.sample(bus);
        self.receiver.ingest_telemetry();
        let delivery = self.link.deliver(ctx, bus, &tx);
        if delivery.downlink_slot {
            if let Some((frame_type, payload)) = self.receiver.next_telemetry() {
                telemetry::apply_frame(&self.fc_telemetry, frame_type, &payload, ctx.time_s);
            }
        }
        self.receiver.emit(&delivery);
        self.outputs.publish(bus, &delivery.report);
        Ok(())
    }
}
```

`crates/ofs-radio/src/lib.rs`:

```rust
//! Radio link models: CRSF framing, the swappable handset / link / receiver parts, and the ExpressLRS link.
pub mod chain;
pub mod crsf;
pub mod elrs;
pub mod joystick;
pub mod parts;
pub mod receiver;
pub mod telemetry;
```

- [ ] **Step 9: Point the test rigs at the chain**

In `crates/ofs-radio/tests/elrs.rs` replace the imports and `rig`:

```rust
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{packet_error_rate, rf_mode_index, sensitivity_dbm, tx_power_index_mw, LinkParams, NOISE_FLOOR_DBM, NO_SIGNAL_RSSI_DBM};
use ofs_radio::receiver::ReceiverParams;
use ofs_radio::telemetry::FcTelemetryHandle;
```
```rust
fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let chain = RadioChain::elrs(params, ReceiverParams::default(), BASE_HZ, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
}
```
Delete the `type ElrsHandle = ...` alias and change `use ofs_radio::elrs::FcTelemetry;` in `mod fc_telemetry` to `use ofs_radio::telemetry::FcTelemetry;`. Any test that read `link_stats_interval_packets` (none do) is unaffected. Fix any other import the compiler flags (`TELEMETRY_FIFO_LEN` now lives in `ofs_radio::receiver`).

In `crates/ofs-radio/tests/golden.rs` replace `rig` and its imports the same way, passing the interval explicitly:

```rust
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::LinkParams;
use ofs_radio::receiver::ReceiverParams;
use ofs_radio::telemetry::FcTelemetryHandle;
```
```rust
fn rig(params: LinkParams, stats_interval: u32, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let receiver = ReceiverParams { link_stats_interval_packets: stats_interval };
    let chain = RadioChain::elrs(params, receiver, BASE_HZ, seed, uart.clone(), telemetry_in.clone(), &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
}
```
Nothing else in `golden.rs` changes, and `tests/golden/elrs.digests` is not touched.

- [ ] **Step 10: Wire the vehicle**

In `crates/ofs-sim/src/vehicle.rs` change the import `use ofs_radio::elrs::{ElrsLink, FcTelemetry, FcTelemetryHandle, LinkParams};` to:

```rust
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{ElrsLink, LinkParams};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::RfFrontEnd;
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};
use ofs_radio::telemetry::{FcTelemetry, FcTelemetryHandle};
```
(drop `ElrsLink`, `JoystickHandset`, `CrsfReceiver` from the import if the builder below does not name them in your arrangement — it does, so keep them). Add this function next to `world_obstacles` (it uses `v3`, `antenna_kind`, `polarization`, `aim_ned`, `world_obstacles`, all private to this file):

```rust
/// The quad file's radio, wired to the flight controller's UART: the joystick handset (at the world's `[handset]`),
/// the ELRS link and the CRSF receiver. `uart` is the receiver's output (the FC's UART RX); `telemetry_in` carries
/// the FC's own CRSF telemetry back.
fn radio_chain(cfg: &QuadConfig, opts: &BuildOptions, base_hz: u32, uart: Wire, telemetry_in: Wire, bus: &mut Bus) -> RadioChain {
    let r = &cfg.radio;
    let facing = opts.world.pilot.facing_deg;
    let handset = &opts.world.handset;
    let front_end = RfFrontEnd {
        position: v3(handset.position(&opts.world.pilot)),
        antennas: handset
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, a.beamwidth_deg),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: aim_ned(facing + a.aim_az_deg, a.aim_el()),
            })
            .collect(),
        tx_power_mw: f64::from(r.tx_power_mw),
    };
    let link = LinkParams {
        packet_rate_hz: r.packet_rate_hz,
        latency_packets: r.latency_packets,
        tx_power_mw: front_end.tx_power_mw,
        handset_position: front_end.position,
        handset_antennas: front_end.antennas.clone(),
        quad_antennas: r
            .antennas
            .iter()
            .map(|a| Antenna {
                kind: antenna_kind(a.kind, None),
                gain_dbi: a.gain_dbi,
                polarization: polarization(a.polarization),
                axis: v3(a.mount_frd).normalize(),
            })
            .collect(),
        obstacles: world_obstacles(&opts.world),
        fading: true,
        ground_bounce: true,
    };
    let handset = JoystickHandset::new(front_end, bus);
    let link = ElrsLink::new(link, opts.seed, bus);
    let receiver = CrsfReceiver::new(ReceiverParams { link_stats_interval_packets: r.link_stats_interval_packets }, uart, telemetry_in);
    RadioChain::new(Box::new(handset), Box::new(link), Box::new(receiver), base_hz, bus)
}
```

Replace, in `build`, everything from `let r = &cfg.radio;` through `models.push(Box::new(elrs));` (the `facing`, `handset`, `link`, `receiver_uart`, `radio_tap`, `elrs`, `fc_telemetry` lines) with:

```rust
    let r = &cfg.radio;
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    let radio_tap = Wire::new(VIDEO_TAP_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    let chain = radio_chain(cfg, opts, base_hz, receiver_uart.clone(), radio_tap.clone(), &mut bus);
    let fc_telemetry = chain.fc_telemetry_handle();
    models.push(Box::new(chain));
```
`r` is still used later (`r.uart`), so keep its binding. Remove any import the compiler now reports unused.

- [ ] **Step 11: Run everything the split touches**

Run: `cargo test -p ofs-radio --locked`
Expected: PASS, including `golden` (identical digests), `elrs`, `crsf`, and the new `parts` tests.

Run: `cargo test -p ofs-sim --test radio_golden --locked`
Expected: PASS (the whole-vehicle MCAP digests are unchanged).

Run: `cargo test --workspace --locked` then `cargo clippy --workspace --all-targets --locked -- -D warnings` then `cargo fmt --all -- --check`
Expected: all green. (Live SITL tests are `#[ignore]` and run in Task 9.)

If a golden digest differs, the split changed behaviour: find the difference (the likeliest causes are a reordered random draw, a changed float expression, or the telemetry-ingest/slot order) and fix the code. **Never regenerate the digests.**

- [ ] **Step 12: Commit**

```bash
git add -A crates/ofs-radio crates/ofs-sim
git commit -m "refactor(radio): split radio.elrs into Handset, Link and Receiver parts

ElrsLink becomes the first Link; the joystick sampling the first Handset; the CRSF writer the first
Receiver. RadioChain steps them in order and the vehicle builds one. Byte-identical: the Task 1 goldens
(UART bytes, signals, telemetry, whole-vehicle MCAP) are unchanged.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---
### Task 3: Band profiles and ELRS 900 MHz

**Files:**
- Create: `crates/ofs-radio/src/band.rs`, `crates/ofs-radio/tests/band.rs`
- Modify: `crates/ofs-radio/src/elrs.rs`, `crates/ofs-radio/src/lib.rs`
- Modify: `crates/ofs-config/src/lib.rs`, `crates/ofs-config/tests/config.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Create: `quads/opendrone-5f-900mhz.toml`, `crates/ofs-sim/tests/radio_chain.rs`

**Interfaces:**
- Consumes: Task 2's `ElrsLink`, `LinkParams`, `RadioChain::elrs`.
- Produces: `ofs_radio::band::{Band, ELRS_900_FREQ_MHZ, NOISE_FLOOR_900_DBM}` with `Band::{ALL, name, from_name, freq_mhz, noise_floor_dbm, packet_rates, sensitivity_dbm, rf_mode_index}`; `LinkParams.band: Band` (default `Band::G2_4` in `ideal`); `ofs_config::BandKind::{G2_4, Mhz900}` with `name()`/`packet_rates()`; `RadioSection.band: Option<BandKind>` and `RadioSection::band_or_default()`.

- [ ] **Step 1: Write the failing band tests**

Create `crates/ofs-radio/tests/band.rs`:

```rust
use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::band::{Band, ELRS_900_FREQ_MHZ, NOISE_FLOOR_900_DBM};
use ofs_radio::chain::RadioChain;
use ofs_radio::elrs::{rf_mode_index, sensitivity_dbm, LinkParams};
use ofs_radio::receiver::ReceiverParams;
use ofs_rf::propagation::fspl_db;

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams) -> Scheduler {
    let mut bus = Bus::new();
    let chain = RadioChain::elrs(params, ReceiverParams::default(), BASE_HZ, 1, Wire::new(1 << 20), Wire::new(1 << 16), &mut bus);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    s
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn set_pos(s: &mut Scheduler, pos: DVec3) {
    let sig = s.bus_mut().lookup::<DVec3>(names::BODY_POS_NED).unwrap();
    s.bus_mut().set(sig, pos);
}

#[test]
fn the_band_tables_are_the_published_rf_modes() {
    assert_eq!(Band::G2_4.packet_rates(), [50, 150, 250, 500]);
    assert_eq!(Band::Mhz900.packet_rates(), [25, 50, 100, 200, 250]);
    for (rate, dbm) in [(25, -123.0), (50, -120.0), (100, -117.0), (200, -112.0), (250, -111.0)] {
        assert_eq!(Band::Mhz900.sensitivity_dbm(rate), Some(dbm), "{rate} Hz");
    }
    assert_eq!(Band::Mhz900.sensitivity_dbm(500), None, "500 Hz is a 2.4 GHz mode");
    assert_eq!(Band::G2_4.sensitivity_dbm(25), None);
}

#[test]
fn the_2_4_ghz_band_is_what_the_free_functions_always_said() {
    for rate in [50, 150, 250, 500] {
        assert_eq!(Band::G2_4.sensitivity_dbm(rate), sensitivity_dbm(rate));
        assert_eq!(Band::G2_4.rf_mode_index(rate), rf_mode_index(rate));
    }
    assert_eq!(Band::G2_4.freq_mhz(), 2440.0);
    assert_eq!(Band::G2_4.noise_floor_dbm(), ofs_radio::elrs::NOISE_FLOOR_DBM);
}

#[test]
fn the_rf_mode_is_the_rates_place_in_the_band_plus_one() {
    assert_eq!(Band::Mhz900.rf_mode_index(25), Some(1));
    assert_eq!(Band::Mhz900.rf_mode_index(250), Some(5));
    assert_eq!(Band::Mhz900.rf_mode_index(300), None);
}

#[test]
fn the_900_mhz_noise_floor_is_the_lora_formula() {
    let formula = -174.0 + 10.0 * 500_000f64.log10() + 6.0;
    assert!((NOISE_FLOOR_900_DBM - formula).abs() < 1e-9, "{NOISE_FLOOR_900_DBM} vs {formula}");
    assert_eq!(Band::Mhz900.noise_floor_dbm(), NOISE_FLOOR_900_DBM);
    assert_eq!(Band::Mhz900.freq_mhz(), ELRS_900_FREQ_MHZ);
}

#[test]
fn names_round_trip_and_unknown_names_are_none() {
    for band in Band::ALL {
        assert_eq!(Band::from_name(band.name()), Some(band));
    }
    assert_eq!(Band::from_name("2g4"), Some(Band::G2_4));
    assert_eq!(Band::from_name("900"), Some(Band::Mhz900));
    assert_eq!(Band::from_name("5g8"), None);
}

#[test]
fn the_900_mhz_budget_at_two_metres_is_free_space_at_915_mhz() {
    let mut s = rig(LinkParams { band: Band::Mhz900, ..LinkParams::ideal(200) });
    set_pos(&mut s, DVec3::new(0.0, 0.0, -1.7)); // level with the handset, 2 m away: dipoles broadside
    s.run_for(0.05).unwrap();
    let expected = 10.0 * 250f64.log10() + 4.0 - fspl_db(2.0, ELRS_900_FREQ_MHZ);
    assert!((get(&s, names::RADIO_RSSI) - expected).abs() < 0.1, "{} vs {expected}", get(&s, names::RADIO_RSSI));
    assert!((get(&s, names::RADIO_SNR) - (expected - NOISE_FLOOR_900_DBM)).abs() < 0.1, "SNR uses the 900 MHz noise floor");
}

#[test]
fn at_40_km_the_900_mhz_link_holds_while_the_2_4_ghz_link_drops_packets() {
    let at_40_km = |params: LinkParams| {
        let mut s = rig(params);
        set_pos(&mut s, DVec3::new(40_000.0, 0.0, -1.7));
        s.run_for(2.0).unwrap();
        get(&s, names::RADIO_LQ)
    };
    let lq_900 = at_40_km(LinkParams { band: Band::Mhz900, ..LinkParams::ideal(100) });
    let lq_2g4 = at_40_km(LinkParams::ideal(500));
    assert_eq!(lq_900, 100.0, "21 dB of margin at 100 Hz");
    assert!(lq_2g4 < 95.0, "the 2.4 GHz link is at its sensitivity edge there: {lq_2g4}");
}
```

- [ ] **Step 2: Run to verify the failure**

Run: `cargo test -p ofs-radio --test band --locked`
Expected: FAIL to compile (`unresolved import ofs_radio::band`).

- [ ] **Step 3: Create `band.rs`**

```rust
//! The ExpressLRS bands the link can run on: carrier frequency, receiver noise floor, and the packet rates each
//! band's LoRa modes offer with their sensitivities (https://expresslrs.org/info/signal-health/).
use crate::elrs::{ELRS_FREQ_MHZ, NOISE_FLOOR_DBM};

/// The middle of the 868/915 MHz band, 915 MHz (the FCC side); frequency hopping is not modelled (estimated).
pub const ELRS_900_FREQ_MHZ: f64 = 915.0;
/// LoRa bandwidth 500 kHz, noise figure 6 dB (estimated): `-174 + 10*log10(500_000) + 6`.
pub const NOISE_FLOOR_900_DBM: f64 = -111.010_299_956_639_81;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Band {
    /// 2.4 GHz LoRa modes: 50, 150, 250 and 500 Hz (the model since M3c).
    #[default]
    G2_4,
    /// 868/915 MHz LoRa modes: 25, 50, 100, 200 and 250 Hz.
    Mhz900,
}

/// (packet rate in Hz, sensitivity in dBm), ascending by rate. ExpressLRS's table lists the 2.4 GHz 50 Hz mode at
/// -115 dBm; this model has used -117 since M3c and changing it would change every seeded 50 Hz run (a roadmap row).
const G2_4_MODES: [(u32, f64); 4] = [(50, -117.0), (150, -112.0), (250, -108.0), (500, -105.0)];
const MHZ_900_MODES: [(u32, f64); 5] = [(25, -123.0), (50, -120.0), (100, -117.0), (200, -112.0), (250, -111.0)];

impl Band {
    pub const ALL: [Band; 2] = [Band::G2_4, Band::Mhz900];

    /// The name a quad file uses: `"2g4"` or `"900"`.
    pub fn name(self) -> &'static str {
        match self {
            Band::G2_4 => "2g4",
            Band::Mhz900 => "900",
        }
    }

    pub fn from_name(name: &str) -> Option<Band> {
        Band::ALL.into_iter().find(|b| b.name() == name)
    }

    pub fn freq_mhz(self) -> f64 {
        match self {
            Band::G2_4 => ELRS_FREQ_MHZ,
            Band::Mhz900 => ELRS_900_FREQ_MHZ,
        }
    }

    pub fn noise_floor_dbm(self) -> f64 {
        match self {
            Band::G2_4 => NOISE_FLOOR_DBM,
            Band::Mhz900 => NOISE_FLOOR_900_DBM,
        }
    }

    fn modes(self) -> &'static [(u32, f64)] {
        match self {
            Band::G2_4 => &G2_4_MODES,
            Band::Mhz900 => &MHZ_900_MODES,
        }
    }

    /// The packet rates this band's LoRa modes offer, ascending.
    pub fn packet_rates(self) -> Vec<u32> {
        self.modes().iter().map(|m| m.0).collect()
    }

    /// The RSSI at which half the packets decode.
    pub fn sensitivity_dbm(self, packet_rate_hz: u32) -> Option<f64> {
        self.modes().iter().find(|m| m.0 == packet_rate_hz).map(|m| m.1)
    }

    /// The CRSF `rf_mode` a receiver reports: the rate's place in the band's ascending list plus one (0 stays the
    /// 4 Hz Crossfire mode). For 2.4 GHz that is 50 -> 1, 150 -> 2, 250 -> 3, 500 -> 4.
    pub fn rf_mode_index(self, packet_rate_hz: u32) -> Option<u8> {
        self.modes().iter().position(|m| m.0 == packet_rate_hz).map(|i| i as u8 + 1)
    }
}
```

Add `pub mod band;` to `lib.rs`.

- [ ] **Step 4: Make the ELRS link band-aware (2.4 GHz unchanged)**

Edit `crates/ofs-radio/src/elrs.rs`:

1. Add `use crate::band::Band;` to the imports.
2. Replace the two free functions so they delegate (their bodies are removed; the signatures and doc comments stay):
```rust
pub fn sensitivity_dbm(packet_rate_hz: u32) -> Option<f64> {
    Band::G2_4.sensitivity_dbm(packet_rate_hz)
}

pub fn rf_mode_index(packet_rate_hz: u32) -> Option<u8> {
    Band::G2_4.rf_mode_index(packet_rate_hz)
}
```
3. In `LinkParams` add, before `latency_packets`:
```rust
    /// The ExpressLRS band: carrier, noise floor and the packet rates and sensitivities of its modes.
    pub band: Band,
```
and in `LinkParams::ideal` add `band: Band::G2_4,` as the first field of the struct literal.
4. In `ElrsLink::new` replace the second assert with:
```rust
        assert!(
            params.band.sensitivity_dbm(params.packet_rate_hz).is_some(),
            "packet_rate_hz {} is not a {} mode: use one of {:?}",
            params.packet_rate_hz,
            params.band.name(),
            params.band.packet_rates()
        );
```
5. In `deliver`, add at the top, after `let transmitting = ...` is computed and before the draws:
```rust
        let freq = self.params.band.freq_mhz();
        let noise = self.params.band.noise_floor_dbm();
```
and replace: every `ELRS_FREQ_MHZ` in `deliver` with `freq`; every `NOISE_FLOOR_DBM` in `deliver` with `noise`; `sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction")` with `self.params.band.sensitivity_dbm(self.params.packet_rate_hz).expect("checked at construction")`; and `rf_mode_index(self.params.packet_rate_hz).unwrap_or(0)` with `self.params.band.rf_mode_index(self.params.packet_rate_hz).unwrap_or(0)`.
6. Update the constants' doc comments: `ELRS_FREQ_MHZ` becomes "The middle of the 2.4 GHz band; frequency hopping is not modelled (the 900 MHz band is in `band`)", unchanged in value.

Update every `LinkParams { ... }` struct literal outside `ideal` (the vehicle builder does this in Step 7) by adding `band`.

- [ ] **Step 5: Run the radio tests**

Run: `cargo test -p ofs-radio --locked`
Expected: PASS: band tests green, the golden test unchanged (2.4 GHz is byte-identical: every value the band returns for `G2_4` is the old constant).

- [ ] **Step 6: The config key**

In `crates/ofs-config/src/lib.rs`, after `RadioKind`, add:

```rust
/// The ExpressLRS band (`kind = "elrs"`); absent means 2.4 GHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum BandKind {
    #[serde(rename = "2g4")]
    G2_4,
    #[serde(rename = "900")]
    Mhz900,
}

impl BandKind {
    pub fn name(self) -> &'static str {
        match self {
            BandKind::G2_4 => "2g4",
            BandKind::Mhz900 => "900",
        }
    }

    /// The packet rates the band's LoRa modes offer. `ofs-radio`'s `Band` is the model's own copy; a test in
    /// `ofs-sim` keeps the two equal.
    pub fn packet_rates(self) -> &'static [u32] {
        match self {
            BandKind::G2_4 => &[50, 150, 250, 500],
            BandKind::Mhz900 => &[25, 50, 100, 200, 250],
        }
    }

    fn label(self) -> &'static str {
        match self {
            BandKind::G2_4 => "2.4 GHz",
            BandKind::Mhz900 => "900 MHz",
        }
    }
}
```

In `RadioSection` add, after `uart`:

```rust
    /// `kind = "elrs"` only: the ExpressLRS band, `"2g4"` (default) or `"900"`; it fixes which packet rates are valid.
    #[serde(default)]
    pub band: Option<BandKind>,
```
and an impl:
```rust
impl RadioSection {
    pub fn band_or_default(&self) -> BandKind {
        self.band.unwrap_or(BandKind::G2_4)
    }
}
```
Update the `packet_rate_hz` doc comment to "The ExpressLRS LoRa modes of the band: 2.4 GHz 50, 150, 250 or 500; 900 MHz 25, 50, 100, 200 or 250 packets per second."

Replace the `packet_rate_hz` validation block in `validate` with:

```rust
        let band = r.band_or_default();
        let rates = band.packet_rates();
        let listed = match rates.split_last() {
            Some((last, rest)) => format!("{} or {last}", rest.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")),
            None => String::new(),
        };
        c.check(
            rates.contains(&r.packet_rate_hz),
            "radio.packet_rate_hz",
            format!("must be one of the ExpressLRS {} LoRa modes: {listed} (got {})", band.label(), r.packet_rate_hz),
        );
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
```
(Keep the existing `c.divides(...)` line only once; the old block's `c.check(matches!(...))` goes.) Export `BandKind` where `RadioKind` is exported (`pub use` list in the crate root if one exists; otherwise it is already `pub` in `lib.rs`).

Add to `crates/ofs-config/tests/config.rs`:

```rust
#[test]
fn the_900_mhz_band_takes_its_own_packet_rates() {
    let ok = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "band = \"900\"\npacket_rate_hz = 200")).unwrap();
    assert_eq!((ok.radio.band_or_default(), ok.radio.packet_rate_hz), (ofs_config::BandKind::Mhz900, 200));
    for rate in [500, 150, 30] {
        let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", &format!("band = \"900\"\npacket_rate_hz = {rate}"))).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("radio.packet_rate_hz") && message.contains("25, 50, 100, 200 or 250"), "{rate}: {message}");
    }
    let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "packet_rate_hz = 200")).unwrap_err();
    assert!(err.to_string().contains("50, 150, 250 or 500"), "the default band is 2.4 GHz: {err}");
}

#[test]
fn an_unknown_band_is_a_parse_error_listing_the_names() {
    let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "band = \"5g8\"\npacket_rate_hz = 500")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `5g8`") && err.to_string().contains("900"), "{err}");
}
```

Run: `cargo test -p ofs-config --locked`
Expected: PASS.

- [ ] **Step 7: Build the band into the vehicle, add the 900 MHz quad and the vehicle tests**

In `vehicle.rs`'s `radio_chain`, add `band` to the `LinkParams` literal (first field) and import `Band`:

```rust
        band: match r.band_or_default() {
            BandKind::G2_4 => Band::G2_4,
            BandKind::Mhz900 => Band::Mhz900,
        },
```
(`use ofs_radio::band::Band;` and `BandKind` from the `ofs_config` import list.)

Create `quads/opendrone-5f-900mhz.toml` as a copy of `quads/opendrone-5f-freestyle.toml` with exactly three edits: line 3 `name = "OpenDrone 5F freestyle (ELRS 900 MHz)"`, and in `[radio]` insert `band = "900"` after `kind = "elrs"` and change `packet_rate_hz = 500` to `packet_rate_hz = 200`; update the `[radio]` comment's first line to `# ExpressLRS 900 MHz (200 Hz LoRa).` The `betaflight_diff` line stays pointing at the freestyle diff, as `weak-link` does.

Create `crates/ofs-sim/tests/radio_chain.rs`:

```rust
//! The radio chain in a whole vehicle: the quad file picks the band; the band tables stay in step with the
//! config's own copy.
use std::path::Path;

use ofs_config::{load, BandKind, FcKind, WorldConfig};
use ofs_radio::band::Band;
use ofs_sim::vehicle::{build, BuildOptions, Vehicle};

fn quad(file: &str) -> Vehicle {
    let path = format!("{}/../../quads/{file}", env!("CARGO_MANIFEST_DIR"));
    let cfg = load(Path::new(&path)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

#[test]
fn the_config_and_the_model_agree_on_every_bands_packet_rates() {
    for (kind, band) in [(BandKind::G2_4, Band::G2_4), (BandKind::Mhz900, Band::Mhz900)] {
        assert_eq!(kind.packet_rates(), band.packet_rates(), "{}", kind.name());
        assert_eq!(kind.name(), band.name());
    }
}

#[test]
fn the_900_mhz_quad_builds_flies_and_keeps_its_link_up() {
    let mut v = quad("opendrone-5f-900mhz.toml");
    v.run_for(1.0).unwrap();
    let radio = v.state().radio;
    assert!(radio.link_up && radio.lq_pct == 100.0, "{radio:?}");
}
```

Run: `cargo test -p ofs-sim --test radio_chain --test radio_golden --locked`
Expected: PASS (the goldens still match; the shipped quads still load).

- [ ] **Step 8: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`
Expected: green.

```bash
git add -A crates quads
git commit -m "feat(radio): ELRS bands - the 900 MHz profile beside 2.4 GHz

Band carries the carrier, noise floor and the per-rate sensitivities (ExpressLRS's published modes). The
link reads them from LinkParams.band; 2.4 GHz is byte-identical. [radio].band picks it, and the valid
packet rates follow the band. Ships quads/opendrone-5f-900mhz.toml.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The `fixed` link

**Files:**
- Create: `crates/ofs-radio/src/fixed.rs`, `crates/ofs-radio/tests/fixed.rs`
- Modify: `crates/ofs-radio/src/lib.rs`, `crates/ofs-radio/src/elrs.rs` (make `TELEMETRY_SLOT_DIVISOR` reachable: it is already `pub const`)
- Modify: `crates/ofs-config/src/lib.rs`, `crates/ofs-config/tests/config.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Create: `quads/opendrone-5f-fixed-link.toml`
- Modify: `crates/ofs-sim/tests/radio_chain.rs`

**Interfaces:**
- Consumes: Task 2's `Link`, `FaultSignals`, `LqWindow`, `RadioChain::new`, `JoystickHandset`, `CrsfReceiver`; `elrs::{NOISE_FLOOR_DBM, NO_SIGNAL_RSSI_DBM, TELEMETRY_SLOT_DIVISOR}`.
- Produces: `ofs_radio::fixed::{FixedLink, FixedParams, MODEL_NAME, NOMINAL_RSSI_DBM, NOMINAL_SNR_DB}`; `FixedLink::new(params: FixedParams, seed: u64, bus: &mut Bus) -> Self`; `ofs_config::{RadioKind::Fixed, FixedSection}` and `RadioSection.fixed: Option<FixedSection>`.

- [ ] **Step 1: Write the failing link tests**

Create `crates/ofs-radio/tests/fixed.rs`:

```rust
use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::chain::RadioChain;
use ofs_radio::crsf::{self, Decoder, Frame};
use ofs_radio::elrs::NO_SIGNAL_RSSI_DBM;
use ofs_radio::fixed::{FixedLink, FixedParams, NOMINAL_RSSI_DBM};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::RfFrontEnd;
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};
use ofs_radio::telemetry::FcTelemetryHandle;

const BASE_HZ: u32 = 8000;
const TICKS_PER_PACKET: u32 = BASE_HZ / 500;

fn params() -> FixedParams {
    FixedParams { packet_rate_hz: 500, latency_packets: 1, loss_fraction: 0.0, burst_on_s: 0.0, burst_off_s: 0.0 }
}

/// The scheduler, the receiver's UART output, the flight controller's telemetry input and the handset's telemetry.
fn rig_full(params: FixedParams, seed: u64) -> (Scheduler, Wire, Wire, FcTelemetryHandle) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let telemetry_in = Wire::new(1 << 16);
    let front_end = RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 };
    let handset = JoystickHandset::new(front_end, &mut bus);
    let link = FixedLink::new(params, seed, &mut bus);
    let receiver = CrsfReceiver::new(ReceiverParams::default(), uart.clone(), telemetry_in.clone());
    let chain = RadioChain::new(Box::new(handset), Box::new(link), Box::new(receiver), BASE_HZ, &mut bus);
    let handle = chain.fc_telemetry_handle();
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));
    (s, uart, telemetry_in, handle)
}

fn rig(params: FixedParams, seed: u64) -> (Scheduler, Wire) {
    let (s, uart, _telemetry_in, _handle) = rig_full(params, seed);
    (s, uart)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

/// Runs `packets` packet slots, one frame-decode per slot: `Some(roll ticks)` when the slot's RC frame arrived.
fn slots(s: &mut Scheduler, uart: &Wire, packets: u32, roll_of: impl Fn(u32) -> f64) -> Vec<Option<u16>> {
    let mut seen = Vec::new();
    for k in 0..packets {
        set(s, names::RC_ROLL, roll_of(k));
        for _ in 0..TICKS_PER_PACKET {
            s.step().unwrap();
        }
        let frames = Decoder::default().push(&uart.take(usize::MAX));
        seen.push(frames.iter().find_map(|f| if let Frame::RcChannels(c) = f { Some(c[0]) } else { None }));
    }
    seen
}

#[test]
fn the_latency_is_exactly_that_many_packets() {
    let (mut s, uart) = rig(FixedParams { latency_packets: 3, ..params() }, 1);
    let roll = |k: u32| f64::from(k) / 100.0;
    let seen = slots(&mut s, &uart, 10, roll);
    for (k, ticks) in seen.iter().enumerate() {
        let sampled = (k as u32).saturating_sub(3);
        assert_eq!(*ticks, Some(crsf::stick_ticks(roll(sampled))), "packet {k} carries the sample from packet {sampled}");
    }
}

#[test]
fn the_loss_fraction_is_seeded_and_close_to_its_value() {
    let delivered = |seed: u64| {
        let (mut s, uart) = rig(FixedParams { loss_fraction: 0.25, ..params() }, seed);
        slots(&mut s, &uart, 4000, |_| 0.0).iter().map(Option::is_some).collect::<Vec<_>>()
    };
    let a = delivered(5);
    let share = a.iter().filter(|d| **d).count() as f64 / a.len() as f64;
    assert!((0.72..0.78).contains(&share), "delivered {share}");
    assert_eq!(a, delivered(5), "the same seed loses the same packets");
    assert_ne!(a, delivered(6), "another seed loses others");
}

#[test]
fn no_loss_delivers_everything_and_total_loss_goes_silent_with_the_floor_published() {
    let (mut s, uart) = rig(params(), 1);
    assert!(slots(&mut s, &uart, 300, |_| 0.0).iter().all(Option::is_some));
    assert_eq!((get(&s, names::RADIO_LINK_UP), get(&s, names::RADIO_LQ), get(&s, names::RADIO_RSSI)), (1.0, 100.0, NOMINAL_RSSI_DBM));
    let (mut s, uart) = rig(FixedParams { loss_fraction: 1.0, ..params() }, 1);
    assert!(slots(&mut s, &uart, 300, |_| 0.0).iter().all(Option::is_none));
    assert_eq!((get(&s, names::RADIO_LINK_UP), get(&s, names::RADIO_LQ), get(&s, names::RADIO_RSSI)), (0.0, 0.0, NO_SIGNAL_RSSI_DBM));
}

#[test]
fn bursts_lose_every_packet_of_their_on_window() {
    let (mut s, uart) = rig(FixedParams { burst_on_s: 0.1, burst_off_s: 0.1, ..params() }, 1);
    let seen = slots(&mut s, &uart, 100, |_| 0.0); // 0.2 s: on for the first 50 packets, off for the next 50
    assert!(seen[..50].iter().all(Option::is_none), "the on window loses everything");
    assert!(seen[50..].iter().all(Option::is_some), "the off window delivers everything");
}

#[test]
fn the_radio_faults_apply_to_a_fixed_link_too() {
    let (mut s, uart) = rig(params(), 1);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_none), "link loss silences it");
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 0.0);
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 1.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_none), "a loss fraction of 1 loses all");
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 0.0);
    assert!(slots(&mut s, &uart, 50, |_| 0.0).iter().all(Option::is_some), "and clearing the faults restores it");
}

#[test]
fn a_telemetry_slot_flies_every_fourth_heard_packet() {
    let (mut s, _uart, telemetry_in, handle) = rig_full(params(), 1);
    // A CRSF battery frame: 24.6 V, 3.2 A, the mAh bytes carrying an index, 100 % remaining.
    let mut battery = vec![0xC8, 10, crsf::FRAMETYPE_BATTERY_SENSOR, 0, 246, 0, 32, 0, 0, 7, 100];
    battery.push(crsf::crc8_dvb_s2(&battery[2..]));
    for _ in 0..8 {
        telemetry_in.write(&battery);
    }
    for _ in 0..(8 * TICKS_PER_PACKET) {
        s.step().unwrap();
    }
    let t = handle.latest();
    assert_eq!(t.frame_count, 2, "8 heard packets: slots on the 4th and 8th");
    assert!((t.battery_v - 24.6).abs() < 1e-9 && (t.battery_a - 3.2).abs() < 1e-9, "{t:?}");
}
```

- [ ] **Step 2: Run to verify the failure**

Run: `cargo test -p ofs-radio --test fixed --locked`
Expected: FAIL to compile (`unresolved import ofs_radio::fixed`).

- [ ] **Step 3: Create `fixed.rs`**

```rust
//! A geometry-free link: a fixed loss fraction, optional bursts and a whole number of packets of latency, with no
//! RF budget. It is the worked example of a [`Link`] in `docs/writing-a-radio-model.md`, and the second,
//! structurally different link that keeps the trait honest.
use std::collections::VecDeque;

use ofs_core::rng::model_rng;
use ofs_core::{Bus, StepCtx};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::elrs::{NOISE_FLOOR_DBM, NO_SIGNAL_RSSI_DBM, TELEMETRY_SLOT_DIVISOR};
use crate::parts::{Channels, Delivery, FaultSignals, Link, LinkReport, LqWindow, TxFrame};

pub const MODEL_NAME: &str = "radio.fixed";
/// What a fixed link reports while it is up: there is no geometry, so nominal numbers (estimated).
pub const NOMINAL_RSSI_DBM: f64 = -50.0;
pub const NOMINAL_SNR_DB: f64 = 40.0;

#[derive(Debug, Clone, PartialEq)]
pub struct FixedParams {
    /// Packets per second; it must divide the base rate.
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// The share of packets lost, in [0, 1].
    pub loss_fraction: f64,
    /// Every packet is lost for `burst_on_s`, then delivered (subject to `loss_fraction`) for `burst_off_s`,
    /// repeating. `burst_on_s = 0` means no bursts.
    pub burst_on_s: f64,
    pub burst_off_s: f64,
}

pub struct FixedLink {
    params: FixedParams,
    rng: ChaCha8Rng,
    fault_rng: ChaCha8Rng,
    faults: FaultSignals,
    pipeline: VecDeque<Channels>,
    history: LqWindow,
    downlink_packets: u32,
}

impl FixedLink {
    pub fn new(params: FixedParams, seed: u64, bus: &mut Bus) -> Self {
        assert!(params.packet_rate_hz > 0, "packet_rate_hz must be positive");
        assert!((0.0..=1.0).contains(&params.loss_fraction), "loss_fraction must be in [0, 1]");
        assert!(
            params.burst_on_s >= 0.0 && params.burst_off_s >= 0.0 && (params.burst_on_s == 0.0 || params.burst_off_s > 0.0),
            "burst windows must be >= 0, and a burst needs an off window"
        );
        Self {
            rng: model_rng(seed, MODEL_NAME),
            fault_rng: model_rng(seed, "fixed-faults"),
            faults: FaultSignals::register(bus),
            pipeline: VecDeque::new(),
            history: LqWindow::new(),
            downlink_packets: 0,
            params,
        }
    }
}

impl Link for FixedLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn packet_rate_hz(&self) -> u32 {
        self.params.packet_rate_hz
    }

    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery {
        // The pipeline delays samples by `latency_packets` packets.
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat_n(tx.channels, self.params.latency_packets as usize));
        }
        self.pipeline.push_back(tx.channels);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        // Both draws happen every packet, whatever the inputs, so a seeded run's stream never shifts.
        let draw: f64 = self.rng.gen();
        let fault_draw: f64 = self.fault_rng.gen();
        let p = &self.params;
        let in_burst = p.burst_on_s > 0.0 && ctx.time_s % (p.burst_on_s + p.burst_off_s) < p.burst_on_s;
        let transmitting = tx.transmitting && !self.faults.link_cut(bus);
        let received = transmitting && draw >= p.loss_fraction && !in_burst && self.faults.survives(bus, ctx.time_s, fault_draw);

        // The downlink is heard whenever the uplink was: one packet in TELEMETRY_SLOT_DIVISOR carries a frame.
        let mut downlink_slot = false;
        if transmitting {
            self.downlink_packets += 1;
            downlink_slot = received && self.downlink_packets % TELEMETRY_SLOT_DIVISOR == 0;
        }

        let lq = self.history.push(received);
        let up = lq > 0.0;
        Delivery {
            channels: received.then_some(channels),
            downlink_slot,
            report: LinkReport {
                link_up: up,
                lq_pct: lq,
                rssi_dbm: if up { NOMINAL_RSSI_DBM } else { NO_SIGNAL_RSSI_DBM },
                snr_db: if up { NOMINAL_SNR_DB } else { NO_SIGNAL_RSSI_DBM - NOISE_FLOOR_DBM },
                antenna: 0,
                downlink_lq_pct: lq,
                uplink_rssi_dbm: [NOMINAL_RSSI_DBM; 2],
                uplink_snr_db: NOMINAL_SNR_DB,
                downlink_rssi_dbm: NOMINAL_RSSI_DBM,
                downlink_snr_db: NOMINAL_SNR_DB,
                rf_mode: 0,
                tx_power_index: 0,
            },
        }
    }
}
```

Add `pub mod fixed;` to `lib.rs`.

- [ ] **Step 4: Run to verify the link passes**

Run: `cargo test -p ofs-radio --test fixed --locked` then `cargo test -p ofs-radio --locked`
Expected: PASS, goldens unchanged.

- [ ] **Step 5: The config**

In `crates/ofs-config/src/lib.rs`:

```rust
pub enum RadioKind {
    /// ExpressLRS, behavioural model (fidelity level 1).
    Elrs,
    /// A geometry-free link: a fixed loss fraction, bursts and latency; no RF budget (`[radio.fixed]`).
    Fixed,
}
```
(add `#[serde(rename_all = "snake_case")]` is already on the enum; the new variant reads `"fixed"`). Add:

```rust
/// `[radio.fixed]`: the parameters of `kind = "fixed"`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedSection {
    /// The share of packets lost, in [0, 1].
    #[serde(default)]
    pub loss_fraction: f64,
    /// Every packet is lost for `burst_on_s`, then delivered for `burst_off_s`, repeating (0 = no bursts).
    #[serde(default)]
    pub burst_on_s: f64,
    #[serde(default)]
    pub burst_off_s: f64,
}
```
In `RadioSection` add after `band`:
```rust
    /// `kind = "fixed"` only: the fixed link's loss and bursts (defaults: a perfect link).
    #[serde(default)]
    pub fixed: Option<FixedSection>,
```
In `validate`, wrap the band rate check so it only applies to `kind = "elrs"`, and add the fixed rules:

```rust
        match r.kind {
            RadioKind::Elrs => {
                let band = r.band_or_default();
                let rates = band.packet_rates();
                let listed = match rates.split_last() {
                    Some((last, rest)) => format!("{} or {last}", rest.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")),
                    None => String::new(),
                };
                c.check(
                    rates.contains(&r.packet_rate_hz),
                    "radio.packet_rate_hz",
                    format!("must be one of the ExpressLRS {} LoRa modes: {listed} (got {})", band.label(), r.packet_rate_hz),
                );
                c.check(r.fixed.is_none(), "radio.fixed", "only applies to kind = \"fixed\"");
            }
            RadioKind::Fixed => {
                c.check(r.band.is_none(), "radio.band", "only applies to kind = \"elrs\"");
                if let Some(f) = &r.fixed {
                    c.check((0.0..=1.0).contains(&f.loss_fraction), "radio.fixed.loss_fraction", format!("must be in [0, 1] (got {})", f.loss_fraction));
                    c.check(
                        f.burst_on_s.is_finite() && f.burst_off_s.is_finite() && f.burst_on_s >= 0.0 && f.burst_off_s >= 0.0,
                        "radio.fixed.burst_on_s",
                        "the burst windows must be finite and >= 0 s",
                    );
                    c.check(f.burst_on_s == 0.0 || f.burst_off_s > 0.0, "radio.fixed.burst_off_s", "a burst needs an off window > 0 s");
                }
                c.check(r.packet_rate_hz > 0, "radio.packet_rate_hz", "must be > 0");
            }
        }
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
```
Update the doc comment of `RadioSection.packet_rate_hz` to mention "(`kind = "fixed"`: any rate that divides the base rate)".

Add to `crates/ofs-config/tests/config.rs`:

```rust
#[test]
fn a_fixed_link_takes_its_own_table_and_any_dividing_rate() {
    let cfg = shipped_with(|t| {
        *t = t
            .replace("kind = \"elrs\"", "kind = \"fixed\"")
            .replace("packet_rate_hz = 500", "packet_rate_hz = 400\n\n[radio.fixed]\nloss_fraction = 0.1\nburst_on_s = 0.05\nburst_off_s = 0.2");
    })
    .unwrap();
    assert_eq!(cfg.radio.kind, RadioKind::Fixed);
    let f = cfg.radio.fixed.unwrap();
    assert_eq!((f.loss_fraction, f.burst_on_s, f.burst_off_s), (0.1, 0.05, 0.2));
}

#[test]
fn keys_that_belong_to_the_other_link_are_errors_not_ignored() {
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"\nband = \"900\"")).unwrap_err();
    assert!(err.to_string().contains("radio.band"), "{err}");
    let err = shipped_with(|t| *t = t.replace("packet_rate_hz = 500", "packet_rate_hz = 500\n\n[radio.fixed]\nloss_fraction = 0.1")).unwrap_err();
    assert!(err.to_string().contains("radio.fixed"), "{err}");
}

#[test]
fn fixed_link_values_are_validated() {
    let with = |fixed: &str| shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"").replace("packet_rate_hz = 500", &format!("packet_rate_hz = 500\n\n[radio.fixed]\n{fixed}")));
    assert!(with("loss_fraction = 1.5").unwrap_err().to_string().contains("radio.fixed.loss_fraction"));
    assert!(with("loss_fraction = -0.1").unwrap_err().to_string().contains("radio.fixed.loss_fraction"));
    assert!(with("burst_on_s = 0.1").unwrap_err().to_string().contains("radio.fixed.burst_off_s"));
    assert!(with("burst_on_s = -1.0").unwrap_err().to_string().contains("radio.fixed.burst_on_s"));
    assert!(with("loss_fraction = 0.0").is_ok());
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"fixed\"").replace("packet_rate_hz = 500", "packet_rate_hz = 300")).unwrap_err();
    assert!(err.to_string().contains("radio.packet_rate_hz"), "300 does not divide 8000: {err}");
}
```

- [ ] **Step 6: Build it into the vehicle, ship the example quad, test a flight**

In `vehicle.rs`, change the link construction in `radio_chain` so the link is a `Box<dyn Link>`:

```rust
    let link: Box<dyn Link> = match r.kind {
        RadioKind::Elrs => Box::new(ElrsLink::new(link_params, opts.seed, bus)),
        RadioKind::Fixed => {
            let f = r.fixed.unwrap_or(FixedSection { loss_fraction: 0.0, burst_on_s: 0.0, burst_off_s: 0.0 });
            Box::new(FixedLink::new(
                FixedParams {
                    packet_rate_hz: r.packet_rate_hz,
                    latency_packets: r.latency_packets,
                    loss_fraction: f.loss_fraction,
                    burst_on_s: f.burst_on_s,
                    burst_off_s: f.burst_off_s,
                },
                opts.seed,
                bus,
            ))
        }
    };
```
(the existing `link` local becomes `link_params`; import `ofs_config::{FixedSection, RadioKind}`, `ofs_radio::fixed::{FixedLink, FixedParams}`, `ofs_radio::parts::Link`.) The `LinkParams` literal is built for both kinds (the front end is shared) but only the elrs arm consumes it.

Create `quads/opendrone-5f-fixed-link.toml` as a copy of `opendrone-5f-freestyle.toml` with: line 3 `name = "OpenDrone 5F freestyle (fixed link: 20 % loss)"`; in `[radio]` the comment `# A geometry-free fixed link (the example in docs/writing-a-radio-model.md): no RF budget, 20 % of packets lost.`, `kind = "fixed"`, and after `tx_power_mw = 250` the table:
```toml

[radio.fixed]
loss_fraction = 0.2
```

Append to `crates/ofs-sim/tests/radio_chain.rs`:

```rust
#[test]
fn the_fixed_link_quad_builds_and_loses_about_a_fifth_of_its_packets() {
    let mut v = quad("opendrone-5f-fixed-link.toml");
    v.run_for(2.0).unwrap();
    let radio = v.state().radio;
    assert!(radio.link_up, "{radio:?}");
    assert!((60.0..=95.0).contains(&radio.lq_pct), "a 20 % loss fraction leaves LQ near 80: {}", radio.lq_pct);
    assert_eq!(radio.rssi_dbm, ofs_radio::fixed::NOMINAL_RSSI_DBM);
}
```

- [ ] **Step 7: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`
Expected: green; the Task 1 goldens untouched.

```bash
git add -A crates quads
git commit -m "feat(radio): the fixed link - loss, bursts and latency with no RF budget

A second, structurally different Link: seeded loss fraction, burst windows, an exact packet latency, the
radio fault catalog honoured. kind = \"fixed\" with [radio.fixed]; keys of the other link are errors.
Ships quads/opendrone-5f-fixed-link.toml.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Part names in the quad file, the swap proof, and "Writing a radio model"

**Files:**
- Modify: `crates/ofs-config/src/lib.rs`, `crates/ofs-config/tests/config.rs`
- Modify: `crates/ofs-sim/src/vehicle.rs`
- Create: `crates/ofs-radio/tests/swap.rs`
- Create: `docs/writing-a-radio-model.md`

**Interfaces:**
- Consumes: Tasks 2-4.
- Produces: `ofs_config::{HandsetKind, ReceiverKind}` (one variant each: `Joystick`, `Crsf`) with `RadioSection.handset` and `.receiver` (defaulted); the swap-proof test; the doc.

- [ ] **Step 1: Write the failing config tests**

Add to `crates/ofs-config/tests/config.rs`:

```rust
#[test]
fn the_handset_and_receiver_default_to_the_joystick_and_crsf_and_can_be_named() {
    let cfg = shipped_with(|_| {}).unwrap();
    assert_eq!((cfg.radio.handset, cfg.radio.receiver), (ofs_config::HandsetKind::Joystick, ofs_config::ReceiverKind::Crsf));
    let named = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nhandset = \"joystick\"\nreceiver = \"crsf\"")).unwrap();
    assert_eq!(named.radio.handset, ofs_config::HandsetKind::Joystick);
}

#[test]
fn an_unknown_part_name_is_an_error_that_lists_the_valid_names() {
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nhandset = \"mixer\"")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `mixer`") && err.to_string().contains("joystick"), "{err}");
    let err = shipped_with(|t| *t = t.replace("kind = \"elrs\"", "kind = \"elrs\"\nreceiver = \"sbus\"")).unwrap_err();
    assert!(err.to_string().contains("unknown variant `sbus`") && err.to_string().contains("crsf"), "{err}");
}
```

Run: `cargo test -p ofs-config --locked`
Expected: FAIL (no `handset` field: "unknown field `handset`", and the missing types).

- [ ] **Step 2: Add the keys**

In `crates/ofs-config/src/lib.rs`:

```rust
/// What reads the sticks (`[radio] handset`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandsetKind {
    /// The stick and switch signals the joystick, the Pilot stream or `SetSticks` drive.
    #[default]
    Joystick,
}

/// What turns the link's output into the flight controller's serial protocol (`[radio] receiver`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiverKind {
    /// CRSF on a flight-controller UART.
    #[default]
    Crsf,
}
```
and in `RadioSection` after `kind`:
```rust
    /// The handset part; the only one today is `"joystick"`.
    #[serde(default)]
    pub handset: HandsetKind,
    /// The receiver part; the only one today is `"crsf"`.
    #[serde(default)]
    pub receiver: ReceiverKind,
```
In `vehicle.rs`'s `radio_chain`, make the part choice explicit so adding a variant is a compile error until handled:

```rust
    let handset: Box<dyn Handset> = match r.handset {
        HandsetKind::Joystick => Box::new(JoystickHandset::new(front_end, bus)),
    };
    let receiver: Box<dyn Receiver> = match r.receiver {
        ReceiverKind::Crsf => Box::new(CrsfReceiver::new(
            ReceiverParams { link_stats_interval_packets: r.link_stats_interval_packets },
            uart,
            telemetry_in,
        )),
    };
    RadioChain::new(handset, link, receiver, base_hz, bus)
```
(import `HandsetKind`, `ReceiverKind`, `ofs_radio::parts::{Handset, Receiver}`; the old `JoystickHandset::new(...)`/`CrsfReceiver::new(...)` local bindings go.)

Run: `cargo test -p ofs-config --locked`
Expected: PASS.

- [ ] **Step 3: Write the swap-proof test**

Create `crates/ofs-radio/tests/swap.rs`:

```rust
//! The swap proof (spec §7.4): a `Link` defined in this file, with no change to the core, builds a chain and
//! drives it through failsafe and recovery.
use glam::DVec3;
use ofs_core::{names, Bus, Scheduler, StepCtx, Wire};
use ofs_radio::chain::RadioChain;
use ofs_radio::crsf::{Decoder, Frame};
use ofs_radio::joystick::JoystickHandset;
use ofs_radio::parts::{Delivery, Link, LinkReport, RfFrontEnd, TxFrame};
use ofs_radio::receiver::{CrsfReceiver, ReceiverParams};

const BASE_HZ: u32 = 8000;

/// Loses every packet for the first `dead_packets`, then delivers every one: the simplest possible outage.
struct OutageLink {
    packets: u32,
    dead_packets: u32,
}

impl Link for OutageLink {
    fn name(&self) -> &str {
        "radio.test-outage"
    }

    fn packet_rate_hz(&self) -> u32 {
        500
    }

    fn deliver(&mut self, _ctx: &StepCtx, _bus: &Bus, tx: &TxFrame) -> Delivery {
        self.packets += 1;
        let up = tx.transmitting && self.packets > self.dead_packets;
        Delivery {
            channels: up.then_some(tx.channels),
            downlink_slot: false,
            report: LinkReport {
                link_up: up,
                lq_pct: if up { 100.0 } else { 0.0 },
                rssi_dbm: -60.0,
                snr_db: 30.0,
                antenna: 0,
                downlink_lq_pct: if up { 100.0 } else { 0.0 },
                uplink_rssi_dbm: [-60.0; 2],
                uplink_snr_db: 30.0,
                downlink_rssi_dbm: -60.0,
                downlink_snr_db: 30.0,
                rf_mode: 0,
                tx_power_index: 0,
            },
        }
    }
}

#[test]
fn a_link_written_outside_the_crate_drives_the_chain_through_an_outage_and_back() {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let front_end = RfFrontEnd { position: DVec3::ZERO, antennas: Vec::new(), tx_power_mw: 250.0 };
    let handset = JoystickHandset::new(front_end, &mut bus);
    let receiver = CrsfReceiver::new(ReceiverParams::default(), uart.clone(), Wire::new(1 << 16));
    let chain = RadioChain::new(Box::new(handset), Box::new(OutageLink { packets: 0, dead_packets: 100 }), Box::new(receiver), BASE_HZ, &mut bus);
    assert_eq!(ofs_core::Model::name(&chain), "radio.test-outage", "the chain takes its link's name");
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(chain));

    s.run_for(0.2).unwrap(); // 100 packets of outage
    assert!(Decoder::default().push(&uart.take(usize::MAX)).is_empty(), "failsafe: the receiver went silent");
    assert_eq!(s.bus().get(s.bus().lookup::<f64>(names::RADIO_LINK_UP).unwrap()), 0.0);

    s.run_for(0.2).unwrap(); // recovery
    let frames = Decoder::default().push(&uart.take(usize::MAX));
    assert_eq!(frames.iter().filter(|f| matches!(f, Frame::RcChannels(_))).count(), 100, "one frame per packet again");
    assert_eq!(s.bus().get(s.bus().lookup::<f64>(names::RADIO_LINK_UP).unwrap()), 1.0);
}
```

Run: `cargo test -p ofs-radio --test swap --locked`
Expected: PASS (it compiles against only the public API: nothing in the crate changes).

- [ ] **Step 4: Write `docs/writing-a-radio-model.md`**

```markdown
# Writing a radio model

The simulated radio is three swappable parts (`crates/ofs-radio/src/parts.rs`): a **handset** (sticks to channels),
a **link** (the air: loss, latency, link quality, the downlink) and a **receiver** (what arrives, as the serial
protocol the flight controller reads, CRSF today). A `RadioChain` steps them in order every packet slot. This page
walks through writing a new **link**, the part you are most likely to want; `crates/ofs-radio/src/fixed.rs` is the
worked example (about 100 lines).

## The contract

```rust
pub trait Link: Send {
    fn name(&self) -> &str;
    fn packet_rate_hz(&self) -> u32;
    fn deliver(&mut self, ctx: &StepCtx, bus: &Bus, tx: &TxFrame) -> Delivery;
}
```

- `deliver` runs once per packet slot (at `packet_rate_hz`, which must divide the base rate). `tx` is what the
  handset sent: 16 CRSF channels as 11-bit ticks and whether the transmitter is on.
- Return `Delivery { channels, downlink_slot, report }`: `channels` is `Some` when the receiver heard the packet (after
  whatever latency you model) and `None` when it was lost; `downlink_slot` is `true` when a telemetry frame flew back
  to the handset this slot; `report` carries the numbers the HUD and Betaflight's LINK_STATISTICS show.
- **Only a link draws random numbers.** Seed a stream with `model_rng(seed, "radio.yours")`, and draw the same count
  of numbers every packet whatever the inputs, so a seeded run never shifts when a fault toggles.
- Read the quad's pose and the radio faults from the bus: register the handles in your constructor
  (`bus.signal(names::BODY_POS_NED)`), and use `FaultSignals` (`link_cut`, `survives`) so the fault catalog's
  `radio_link_loss`, `radio_loss` and `burst_loss` work on your link for free. `LqWindow` gives the last-100-packets
  link quality ExpressLRS reports.

## Wiring it in

1. Add a module (`crates/ofs-radio/src/yours.rs`) and `pub mod yours;` in `lib.rs`.
2. Add a variant to `RadioKind` in `crates/ofs-config/src/lib.rs`, a table for its parameters (see `FixedSection`),
   and its validation next to the `RadioKind::Fixed` arm in `validate` (unknown keys are errors by default; keys that
   belong to another link are rejected there).
3. Add the match arm in `radio_chain` in `crates/ofs-sim/src/vehicle.rs`: the compiler points at it, because the
   `match` over `RadioKind` is exhaustive.
4. Write tests like `crates/ofs-radio/tests/fixed.rs`: exact latency, seeded loss, faults honoured. A link defined
   inside a test file works too: see `crates/ofs-radio/tests/swap.rs`.
5. Ship an example quad (`quads/opendrone-5f-fixed-link.toml`) so the model is one `load` away.

## Determinism

The radio goldens (`crates/ofs-radio/tests/golden.rs`, `crates/ofs-sim/tests/radio_golden.rs`) pin the ELRS link's
bytes. They are for the ELRS link: a new link gets its own tests, and nothing it does may change the ELRS digests.
```

- [ ] **Step 5: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`
Expected: green.

```bash
git add -A crates docs
git commit -m "feat(radio): handset and receiver are named in the quad file; the swap is proven

[radio] handset/receiver (one variant each today) make the chain's composition explicit and the match
exhaustive. A Link defined in a test file drives a chain through an outage and recovery. docs/
writing-a-radio-model.md walks through adding a link.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---
### Task 6: The inputs recorder

**Files:**
- Create: `crates/ofs-sim/src/inputs_recorder.rs`, `crates/ofs-sim/tests/inputs_recording.rs`
- Modify: `crates/ofs-sim/src/lib.rs`, `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs`
- Modify: `crates/ofs-log/src/recorder.rs`, `crates/ofs-log/src/lib.rs` (export), `crates/ofs-log/tests/recorder.rs`

**Interfaces:**
- Consumes: `ofs_log::RecordError`; `Vehicle::{set_sticks, play_inputs, run_for, digest, start_recording}`; `ofs_sim::inputs::parse`.
- Produces:
  - `ofs_log::recorder::next_numbered_path(dir: &Path, stem: &str, ext: &str) -> std::io::Result<PathBuf>`
  - `ofs_sim::inputs_recorder::{InputsRecorder, InputsRecordingSummary { path: PathBuf, rows: u64, duration_s: f64 }, HEADER}`
  - `Vehicle::{start_inputs_recording(&mut self, path: &Path) -> Result<(), RecordError>, stop_inputs_recording(&mut self) -> Result<InputsRecordingSummary, RecordError>, is_recording_inputs(&self) -> bool, take_inputs_recording_failure(&mut self) -> Option<String>}`

- [ ] **Step 1: Number any file kind**

In `crates/ofs-log/src/recorder.rs` replace `next_log_path` (keep its doc comment on the first function) with:

```rust
pub fn next_log_path(dir: &Path, stem: &str) -> std::io::Result<PathBuf> {
    next_numbered_path(dir, stem, "mcap")
}

/// `<dir>/<stem>-<n>.<ext>`, with `n` one past the largest number any `<stem>-<n>.<ext>` in `dir` has (the
/// directory is created if missing).
pub fn next_numbered_path(dir: &Path, stem: &str, ext: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let prefix = format!("{stem}-");
    let suffix = format!(".{ext}");
    let mut largest = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if let Some(n) = name.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(&suffix)).and_then(|n| n.parse::<u64>().ok()) {
            largest = largest.max(n);
        }
    }
    Ok(dir.join(format!("{stem}-{}{suffix}", largest + 1)))
}
```
Export it from `ofs-log`'s `lib.rs` wherever `next_log_path` is exported (it is reached as `ofs_log::recorder::next_log_path`; `recorder` is already a public module). Add to `crates/ofs-log/tests/recorder.rs`:

```rust
#[test]
fn numbered_paths_count_only_their_own_stem_and_extension() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    assert_eq!(next_numbered_path(&logs, "quad-inputs", "csv").unwrap(), logs.join("quad-inputs-1.csv"));
    for name in ["quad-inputs-3.csv", "quad-inputs-7.mcap", "quad-4.csv", "quad-inputs-x.csv"] {
        std::fs::write(logs.join(name), "").unwrap();
    }
    assert_eq!(next_numbered_path(&logs, "quad-inputs", "csv").unwrap(), logs.join("quad-inputs-4.csv"));
    assert_eq!(next_log_path(&logs, "quad-inputs").unwrap(), logs.join("quad-inputs-8.mcap"));
}
```
(import `next_numbered_path` beside `next_log_path` in that test file.) Run `cargo test -p ofs-log --locked` — expected PASS.

- [ ] **Step 2: Write the failing recorder tests**

Create `crates/ofs-sim/tests/inputs_recording.rs`:

```rust
//! Recording a live flight's sticks to a `play_inputs` CSV (spec §6): rows only on change, one row per tick, only
//! playable values, a file that is complete however the session ends, and a lockstep replay that reproduces the
//! flight.
use std::path::Path;

use ofs_config::{load, FcKind, WorldConfig};
use ofs_log::RecordError;
use ofs_sim::inputs;
use ofs_sim::vehicle::{build, BuildOptions, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
const HEADER: &str = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4";

fn vehicle() -> Vehicle {
    let cfg = load(Path::new(QUAD)).unwrap();
    let opts = BuildOptions { seed: 1, data_dir: std::env::temp_dir().join("ofs-test-data"), fc_override: Some(FcKind::OpenLoop), world: WorldConfig::open_field() };
    build(&cfg, &opts).unwrap()
}

fn lines(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path).unwrap().lines().map(String::from).collect()
}

fn sticks(throttle: f64, roll: f64) -> Sticks {
    Sticks { throttle, roll, ..Sticks::default() }
}

#[test]
fn rows_are_written_only_when_a_stick_changes_and_the_first_row_is_complete() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.set_sticks(&sticks(0.5, 0.0)); // the same tick as the start: it replaces the initial row
    v.run_for(0.02).unwrap();
    v.set_sticks(&sticks(0.5, 0.0)); // unchanged: no row
    v.run_for(0.02).unwrap();
    v.set_sticks(&sticks(0.5, 0.25));
    v.run_for(0.01).unwrap();
    let summary = v.stop_inputs_recording().unwrap();
    assert_eq!(
        lines(&path),
        [HEADER, "0,0,0,0,0.5,-1,-1,-1,-1", "0.04,0.25,0,0,0.5,-1,-1,-1,-1"],
        "the initial sticks, then the change at tick 320 (0.04 s)"
    );
    assert_eq!((summary.rows, summary.duration_s, summary.path), (2, 0.04, path));
}

#[test]
fn two_changes_in_one_tick_leave_the_later_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.run_for(0.01).unwrap();
    v.start_inputs_recording(&path).unwrap();
    v.set_sticks(&sticks(0.1, 0.0));
    v.set_sticks(&sticks(0.2, 0.0));
    v.set_sticks(&sticks(0.3, 0.0));
    v.stop_inputs_recording().unwrap();
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0.3,-1,-1,-1,-1"], "t_s counts from the start of the recording");
}

#[test]
fn a_non_finite_stick_adds_no_row_and_an_out_of_range_one_is_clamped_so_the_file_stays_playable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.01).unwrap();
    v.set_sticks(&sticks(f64::NAN, 0.0));
    v.run_for(0.01).unwrap();
    v.set_sticks(&Sticks { roll: 3.0, throttle: -2.0, ..Sticks::default() });
    v.run_for(0.01).unwrap();
    v.stop_inputs_recording().unwrap();
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0,-1,-1,-1,-1", "0.02,1,0,0,0,-1,-1,-1,-1"]);
    inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).expect("every value is inside the playable ranges");
}

#[test]
fn a_recording_that_never_changes_is_one_row_and_still_parses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.5).unwrap();
    let summary = v.stop_inputs_recording().unwrap();
    assert_eq!((summary.rows, summary.duration_s), (1, 0.0));
    assert_eq!(inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).unwrap().rows.len(), 1);
}

#[test]
fn a_vehicle_dropped_mid_recording_leaves_a_complete_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    {
        let mut v = vehicle();
        v.start_inputs_recording(&path).unwrap();
        v.run_for(0.01).unwrap();
        v.set_sticks(&sticks(0.7, 0.0)); // the newest row is still pending when the session ends
    }
    assert_eq!(lines(&path), [HEADER, "0,0,0,0,0,-1,-1,-1,-1", "0.01,0,0,0,0.7,-1,-1,-1,-1"]);
}

#[test]
fn recordings_start_once_stop_once_and_refuse_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let mut v = vehicle();
    assert_eq!(v.stop_inputs_recording().unwrap_err(), RecordError::NotRecording);
    v.start_inputs_recording(&path).unwrap();
    assert!(v.is_recording_inputs());
    assert_eq!(v.start_inputs_recording(&dir.path().join("other.csv")).unwrap_err(), RecordError::AlreadyRecording);
    v.stop_inputs_recording().unwrap();
    assert!(!v.is_recording_inputs());
    assert!(matches!(v.start_inputs_recording(&path), Err(RecordError::Io(_))), "it never truncates an earlier file");
    assert!(v.take_inputs_recording_failure().is_none());
}

#[test]
fn floats_round_trip_exactly_through_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sticks.csv");
    let awkward = 0.1 + 0.2; // 0.30000000000000004
    let mut v = vehicle();
    v.start_inputs_recording(&path).unwrap();
    v.run_for(0.01).unwrap();
    v.set_sticks(&sticks(1.0 / 3.0, awkward));
    v.stop_inputs_recording().unwrap();
    let parsed = inputs::parse("sticks.csv", &std::fs::read_to_string(&path).unwrap()).unwrap();
    let last = parsed.rows.last().unwrap();
    assert_eq!(last.1[0], awkward, "roll");
    assert_eq!(last.1[3], 1.0 / 3.0, "throttle");
}

/// The flight of the replay tests: three stick changes, 2.2 s.
fn flown(record_to: Option<&Path>) -> Vehicle {
    let mut v = vehicle();
    if let Some(path) = record_to {
        v.start_inputs_recording(path).unwrap();
    }
    for (throttle, roll, secs) in [(0.0, 0.0, 0.5), (0.6, 0.0, 1.0), (0.4, 0.5, 0.7)] {
        v.set_sticks(&sticks(throttle, roll));
        v.run_for(secs).unwrap();
    }
    if record_to.is_some() {
        v.stop_inputs_recording().unwrap();
    }
    v
}

fn replayed(csv: &str) -> Vehicle {
    let mut v = vehicle();
    v.play_inputs(inputs::parse("recorded.csv", csv).unwrap()).unwrap();
    v.run_for(2.2).unwrap();
    v
}

#[test]
fn a_lockstep_replay_reproduces_the_recorded_flight_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flight.csv");
    let original = flown(Some(&path));
    let csv = std::fs::read_to_string(&path).unwrap();
    assert_eq!(csv.lines().count(), 1 + 3, "the header and the three changes: {csv}");
    assert_eq!(replayed(&csv).digest(), original.digest(), "the same sticks on the same ticks: the same flight");
    assert_eq!(flown(None).digest(), original.digest(), "recording the sticks never changes the flight");
}

#[test]
fn two_replays_write_byte_identical_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let flight = dir.path().join("flight.csv");
    flown(Some(&flight));
    let csv = std::fs::read_to_string(&flight).unwrap();
    let mcap = |name: &str| {
        let path = dir.path().join(name);
        let mut v = vehicle();
        v.start_recording(&[], &path).unwrap();
        v.play_inputs(inputs::parse("recorded.csv", &csv).unwrap()).unwrap();
        v.run_for(2.2).unwrap();
        v.stop_recording().unwrap();
        std::fs::read(path).unwrap()
    };
    let (a, b) = (mcap("a.mcap"), mcap("b.mcap"));
    assert!(a.len() > 10_000 && a == b, "{} vs {} bytes", a.len(), b.len());
}
```

- [ ] **Step 3: Run to verify the failure**

Run: `cargo test -p ofs-sim --test inputs_recording --locked`
Expected: FAIL to compile (`no method named start_inputs_recording`).

- [ ] **Step 4: Write the recorder**

Create `crates/ofs-sim/src/inputs_recorder.rs`:

```rust
//! Records the sticks of a live flight as a `play_inputs` CSV (spec §6): a row whenever any stick changes, in the
//! columns `inputs::parse` reads, so the file replays the flight's sticks tick for tick in lockstep.
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use ofs_core::names::RC_AUX_COUNT;
use ofs_log::RecordError;

use crate::vehicle::Sticks;

pub const HEADER: &str = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4";

const _: () = assert!(RC_AUX_COUNT == 4, "the CSV header names four aux columns");

/// What a finished recording wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct InputsRecordingSummary {
    pub path: PathBuf,
    pub rows: u64,
    /// The time of the last row, in seconds from the start of the recording.
    pub duration_s: f64,
}

pub struct InputsRecorder {
    out: BufWriter<std::fs::File>,
    path: PathBuf,
    base_hz: u32,
    start_tick: u64,
    /// The newest row, held until the tick moves on: a second change in the same tick replaces it.
    pending: Option<(u64, [f64; 8])>,
    /// The last values seen (written or pending): setting the sticks to the same values adds no row.
    last: Option<[f64; 8]>,
    rows: u64,
    last_row_ticks: u64,
    failure: Option<String>,
}

/// The eight values of a row (roll, pitch, yaw, throttle, aux1..4), clamped to the ranges `play_inputs` accepts
/// (throttle [0, 1], every other stick [-1, 1]); `None` when any is not finite (such a value never reaches the file).
fn values_of(s: &Sticks) -> Option<[f64; 8]> {
    let raw = [s.roll, s.pitch, s.yaw, s.throttle, s.aux[0], s.aux[1], s.aux[2], s.aux[3]];
    if raw.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut v = raw.map(|x| x.clamp(-1.0, 1.0));
    v[3] = raw[3].clamp(0.0, 1.0);
    Some(v)
}

impl InputsRecorder {
    /// Creates `path` (never truncating an existing file), writes the header and the first row: the sticks as they
    /// stand at `tick`.
    pub fn start(path: &Path, base_hz: u32, tick: u64, sticks: &Sticks) -> Result<Self, RecordError> {
        let io = |e: std::io::Error| RecordError::Io(format!("{}: {e}", path.display()));
        let file = OpenOptions::new().write(true).create_new(true).open(path).map_err(io)?;
        let mut out = BufWriter::new(file);
        writeln!(out, "{HEADER}").map_err(io)?;
        let mut recorder =
            Self { out, path: path.to_owned(), base_hz, start_tick: tick, pending: None, last: None, rows: 0, last_row_ticks: 0, failure: None };
        recorder.record(tick, sticks);
        Ok(recorder)
    }

    /// Notes the sticks as set at `tick`: a new row when they differ from the last values seen.
    pub fn record(&mut self, tick: u64, sticks: &Sticks) {
        let Some(values) = values_of(sticks) else { return };
        if self.last == Some(values) {
            return;
        }
        self.last = Some(values);
        let since_start = tick.saturating_sub(self.start_tick);
        match &mut self.pending {
            Some((t, v)) if *t == since_start => *v = values,
            _ => {
                self.write_pending();
                self.pending = Some((since_start, values));
            }
        }
    }

    fn write_pending(&mut self) {
        let Some((ticks, values)) = self.pending.take() else { return };
        if self.failure.is_some() {
            return;
        }
        let t_s = ticks as f64 / f64::from(self.base_hz);
        let columns = values.map(|v| v.to_string()).join(",");
        match writeln!(self.out, "{t_s},{columns}") {
            Ok(()) => {
                self.rows += 1;
                self.last_row_ticks = ticks;
            }
            Err(e) => self.failure = Some(format!("{}: {e}", self.path.display())),
        }
    }

    /// Why writing failed, once it has; the recording is dead from then on.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// Writes the pending row and flushes the file.
    pub fn finish(mut self) -> Result<InputsRecordingSummary, RecordError> {
        self.write_pending();
        let flushed = self.out.flush();
        if let Some(message) = self.failure.take() {
            return Err(RecordError::Io(message));
        }
        flushed.map_err(|e| RecordError::Io(format!("{}: {e}", self.path.display())))?;
        Ok(InputsRecordingSummary {
            path: self.path.clone(),
            rows: self.rows,
            duration_s: self.last_row_ticks as f64 / f64::from(self.base_hz),
        })
    }
}

impl Drop for InputsRecorder {
    /// A session that ends mid-recording still leaves a complete file.
    fn drop(&mut self) {
        self.write_pending();
        let _ = self.out.flush();
    }
}
```

Add `pub mod inputs_recorder;` to `crates/ofs-sim/src/lib.rs`.

- [ ] **Step 5: Hook it into the vehicle**

In `crates/ofs-sim/src/vehicle.rs`:

1. Imports: `use crate::inputs_recorder::{InputsRecorder, InputsRecordingSummary};` (`RecordError` and `Path` are already imported for `start_recording`).
2. Add the field to `Vehicle`: `inputs_recorder: Option<InputsRecorder>,` and `inputs_recorder: None,` in the literal in `build`.
3. At the end of `set_sticks` (after the aux loop, outside the `bus` borrow):
```rust
        if let Some(recorder) = &mut self.inputs_recorder {
            recorder.record(self.scheduler.tick(), s);
        }
```
4. Add next to `start_recording`:
```rust
    /// The sticks as the bus holds them now.
    fn sticks_now(&self) -> Sticks {
        let (h, bus) = (&self.h, self.scheduler.bus());
        let mut aux = [0.0; names::RC_AUX_COUNT];
        for (a, s) in aux.iter_mut().zip(&h.aux) {
            *a = bus.get(*s);
        }
        Sticks { roll: bus.get(h.roll), pitch: bus.get(h.pitch), yaw: bus.get(h.yaw), throttle: bus.get(h.throttle), aux }
    }

    /// Starts recording the sticks to a new CSV at `path` (the server names it): a row whenever they change, in
    /// `play_inputs`' format. One at a time; an existing file is never truncated.
    pub fn start_inputs_recording(&mut self, path: &Path) -> Result<(), RecordError> {
        if self.inputs_recorder.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        let now = self.sticks_now();
        self.inputs_recorder = Some(InputsRecorder::start(path, self.scheduler.base_hz(), self.scheduler.tick(), &now)?);
        Ok(())
    }

    /// Finishes the running inputs recording's file.
    pub fn stop_inputs_recording(&mut self) -> Result<InputsRecordingSummary, RecordError> {
        self.inputs_recorder.take().ok_or(RecordError::NotRecording)?.finish()
    }

    pub fn is_recording_inputs(&self) -> bool {
        self.inputs_recorder.is_some()
    }

    /// Why the inputs recording died on its own (a write failed), reported once; the recording is over.
    pub fn take_inputs_recording_failure(&mut self) -> Option<String> {
        let message = self.inputs_recorder.as_ref()?.failure()?.to_owned();
        self.inputs_recorder = None;
        Some(message)
    }
```
(The session's event for a failed recording is added in Task 7, where the event kinds exist.)

- [ ] **Step 6: Run to verify**

Run: `cargo test -p ofs-sim --test inputs_recording --locked` then `cargo test -p ofs-sim --test radio_golden --locked`
Expected: PASS; goldens unchanged.

If `a_lockstep_replay_reproduces_the_recorded_flight_exactly` fails on the digest, the cause is a tick-alignment difference between `set_sticks` before a step and `apply_due` at a row's tick: print `original.digest()`, `replayed(...).digest()` and the CSV, find the first tick whose `rc.*` signals differ (`Vehicle::digest` after `step_ticks(1)` loops on both), and fix the recorder's tick (the row must carry the tick at which `set_sticks` ran, `scheduler.tick()` before the next step).

- [ ] **Step 7: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`
Expected: green.

```bash
git add -A crates
git commit -m "feat(sim): record a live flight's sticks to a play_inputs CSV

An InputsRecorder at Vehicle::set_sticks writes a row whenever a stick changes (one per tick, clamped to the
playable ranges, non-finite values skipped, round-trip floats), and a session that ends mid-recording still
leaves a complete file. A lockstep replay reproduces the recorded flight's digest, and two replays write
byte-identical MCAP.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Protocol 10 — the RPCs, the server, the Python client

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`
- Modify: `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/src/session.rs`, `crates/ofs-sim/tests/grpc.rs`
- Regenerate: `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py`
- Modify: `python/ofs/client.py`, create `python/tests/test_inputs_recording.py`

**Interfaces:**
- Consumes: Task 6's `Vehicle::{start_inputs_recording, stop_inputs_recording, is_recording_inputs, take_inputs_recording_failure}`, `next_numbered_path`.
- Produces: RPCs `StartInputsRecording(Empty) -> InputsRecordingStarted { path }`, `StopInputsRecording(Empty) -> InputsRecordingSummary { path, rows, duration_s }`; events `INPUTS_RECORDING_STARTED = 20` (message: the path) and `INPUTS_RECORDING_STOPPED = 21` (message: `<path>: <rows> rows over <duration> s`, or `inputs recording failed: ...`); Python `Sim.record_inputs() -> str` and `Sim.stop_inputs_recording() -> str`; `PROTOCOL_VERSION = 10` on both sides.

- [ ] **Step 1: Write the failing server tests**

Add to `crates/ofs-sim/tests/grpc.rs` (after `the_server_names_recordings_and_reports_them_as_events`; add `InputsRecordingStarted`-free imports: only `Empty`, `RunRequest`, `Sticks`, `EventKind` and `InputsFile` are used, all imported already):

```rust
#[tokio::test]
async fn the_server_records_a_sessions_sticks_names_the_file_and_reports_it() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    assert_eq!(kind(&c.stop_inputs_recording(Empty {}).await.unwrap_err()), "invalid_state", "none running");
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert_eq!(std::path::PathBuf::from(&started.path), service.logs_dir().join("opendrone-5f-freestyle-inputs-1.csv"));
    assert_eq!(kind(&c.start_inputs_recording(Empty {}).await.unwrap_err()), "invalid_state", "one at a time");
    c.set_sticks(Sticks { throttle: 0.5, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    c.set_sticks(Sticks { throttle: 0.8, roll: 0.25, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    let summary = c.stop_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert_eq!((summary.path.as_str(), summary.rows, summary.duration_s), (started.path.as_str(), 2, 0.5));
    let text = std::fs::read_to_string(&started.path).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ["t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4", "0,0,0,0,0.5,-1,-1,-1,-1", "0.5,0.25,0,0,0.8,-1,-1,-1,-1"]
    );
    let next = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    assert!(next.path.ends_with("opendrone-5f-freestyle-inputs-2.csv"), "{}", next.path);
    let got = drain(&mut events, &[EventKind::InputsRecordingStarted, EventKind::InputsRecordingStopped]);
    assert_eq!(
        got,
        [
            (EventKind::InputsRecordingStarted, started.path.clone()),
            (EventKind::InputsRecordingStopped, format!("{}: 2 rows over 0.5 s", started.path)),
            (EventKind::InputsRecordingStarted, next.path.clone()),
        ]
    );
}

#[tokio::test]
async fn an_inputs_recording_is_refused_while_a_file_plays_and_without_a_session() {
    let mut c = start().await;
    assert_eq!(kind(&c.start_inputs_recording(Empty {}).await.unwrap_err()), "not_loaded");
    load_open_loop(&mut c).await;
    c.play_inputs(InputsFile { name: "climb.csv".into(), csv: "t_s,throttle\n0,0.3\n30,1\n".into() }).await.unwrap();
    let err = c.start_inputs_recording(Empty {}).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    assert!(err.message().contains("climb.csv is playing"), "{}", err.message());
}

#[tokio::test]
async fn unloading_mid_recording_leaves_a_complete_file() {
    let (mut c, service) = start_with_service().await;
    load_open_loop(&mut c).await;
    let started = c.start_inputs_recording(Empty {}).await.unwrap().into_inner();
    c.set_sticks(Sticks { throttle: 0.3, ..Default::default() }).await.unwrap();
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    c.unload(Empty {}).await.unwrap();
    let text = std::fs::read_to_string(&started.path).unwrap();
    assert_eq!(text.lines().collect::<Vec<_>>(), ["t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4", "0,0,0,0,0.3,-1,-1,-1,-1"]);
    let _ = service;
}
```
(If the error kind string for "no session" differs from `not_loaded`, use the one `loaded()` produces: `grep -n 'fn loaded' crates/ofs-sim/src/server.rs`.)

Run: `cargo test -p ofs-sim --test grpc --locked`
Expected: FAIL to compile (the RPCs do not exist).

- [ ] **Step 2: The protocol**

`proto/ofs/v1/sim.proto`: after `rpc StopInputs(Empty) returns (Empty);` add

```proto
  // Records the session's sticks to a stick-inputs CSV the server names (<data_dir>/logs/<quad>-inputs-<n>.csv): a row
  // whenever a stick changes, in PlayInputs' format, so PlayInputs replays the flight's sticks in lockstep. One at a
  // time; refused while an inputs file plays. The file is finished by StopInputsRecording, Unload, a new Load or the
  // session ending.
  rpc StartInputsRecording(Empty) returns (InputsRecordingStarted);
  // Finishes the running inputs recording's file.
  rpc StopInputsRecording(Empty) returns (InputsRecordingSummary);
```
after `message InputsFile {...}` add

```proto
message InputsRecordingStarted {
  string path = 1;
}

message InputsRecordingSummary {
  string path = 1;
  uint64 rows = 2;
  double duration_s = 3;  // the time of the last row
}
```
and after `EVENT_KIND_RECORDING_STOPPED = 19;` add

```proto
  EVENT_KIND_INPUTS_RECORDING_STARTED = 20;  // the message is the file's path
  EVENT_KIND_INPUTS_RECORDING_STOPPED = 21;  // stopped, or failed (a disk error): the message says which
```
`crates/ofs-proto/src/lib.rs`: `PROTOCOL_VERSION: u32 = 10`. Find every other place that hard-codes the number: `grep -rn "PROTOCOL_VERSION\|protocol 9\|protocol_version" crates python docs --include=*.rs --include=*.py --include=*.md` and update tests that assert 9.

- [ ] **Step 3: The server**

In `crates/ofs-sim/src/server.rs`: add `use ofs_log::recorder::next_numbered_path;` beside the existing `next_log_path` import, and this mapper next to `record_error`:

```rust
fn inputs_record_error(e: RecordError) -> Status {
    match e {
        RecordError::AlreadyRecording => invalid_state("an inputs recording is already running; stop it first"),
        RecordError::NotRecording => invalid_state("no inputs recording is running"),
        RecordError::Spec(m) => error("invalid_argument", Code::InvalidArgument, m),
        RecordError::Io(_) => error("internal", Code::Internal, e.to_string()),
    }
}
```
and, after `stop_inputs`, the two RPCs:

```rust
    async fn start_inputs_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::InputsRecordingStarted>, Status> {
        let logs = self.logs_dir();
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if s.vehicle.is_recording_inputs() {
                return Err(inputs_record_error(RecordError::AlreadyRecording));
            }
            if let Some(playing) = s.vehicle.inputs_playing() {
                return Err(invalid_state(&format!("{playing} is playing scripted inputs: there are no live sticks to record")));
            }
            let stem = format!("{}-inputs", s.quad_stem);
            let path = next_numbered_path(&logs, &stem, "csv")
                .map_err(|e| inputs_record_error(RecordError::Io(format!("{}: {e}", logs.display()))))?;
            s.vehicle.start_inputs_recording(&path).map_err(inputs_record_error)?;
            let path = path.display().to_string();
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsRecordingStarted, path.clone())]);
            Ok(pb::InputsRecordingStarted { path })
        })
        .await
    }

    async fn stop_inputs_recording(&self, _req: Request<pb::Empty>) -> Result<Response<pb::InputsRecordingSummary>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            let summary = s.vehicle.stop_inputs_recording().map_err(inputs_record_error)?;
            let path = summary.path.display().to_string();
            let message = format!("{path}: {} rows over {} s", summary.rows, summary.duration_s);
            shared.publish([event(s.vehicle.time_s(), pb::EventKind::InputsRecordingStopped, message)]);
            Ok(pb::InputsRecordingSummary { path, rows: summary.rows, duration_s: summary.duration_s })
        })
        .await
    }
```
`RecordError` is already imported in `server.rs` (used by `record_error`). In `crates/ofs-sim/src/session.rs::changes`, after the `take_recording_failure` block, add:

```rust
        if let Some(message) = self.vehicle.take_inputs_recording_failure() {
            out.push(event(s.time_s, pb::EventKind::InputsRecordingStopped, format!("inputs recording failed: {message}")));
        }
```
Run: `cargo test -p ofs-sim --locked`
Expected: PASS, including the three new grpc tests. (A Load that replaces the session, and `Unload`, drop the old `Vehicle`, whose recorder's `Drop` finishes the file.)

- [ ] **Step 4: Regenerate the Python stubs and add the client methods**

Run the recipe from `docs/dev-setup.md`: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` (install `python -m pip install -e "python[dev]"` first if `grpc_tools` is missing). In `python/ofs/client.py` set `PROTOCOL_VERSION = 10` and add, after `stop_inputs`:

```python
    def record_inputs(self) -> str:
        """Starts recording this session's sticks to a CSV the server names under `logs_dir` (one row whenever a
        stick changes, in `play_inputs`' format) and returns its path. Fly the session (the Godot game's sticks, or
        `set_sticks` calls) and finish with `stop_inputs_recording()`; `play_inputs(path)` then replays the sticks in
        a lockstep session. One recording at a time; refused while an inputs file plays."""
        return self._call(self._stub.StartInputsRecording, pb.Empty()).path

    def stop_inputs_recording(self) -> str:
        """Finishes the inputs recording and returns its path (the file is complete from then on)."""
        return self._call(self._stub.StopInputsRecording, pb.Empty()).path
```
Create `python/tests/test_inputs_recording.py`:

```python
"""Recording a session's sticks and replaying them (spec §6), open loop: no Betaflight needed."""
import pytest

import ofs
from conftest import QUAD

HEADER = "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4"


def test_a_recorded_flight_replays_to_the_same_motor_commands(sim):
    sim.load(QUAD, open_loop_fc=True)
    path = sim.record_inputs()
    sim.set_sticks(throttle=0.3)
    first = sim.run(0.5).motor_cmd
    sim.set_sticks(throttle=0.9, roll=0.2)
    second = sim.run(0.5).motor_cmd
    assert sim.stop_inputs_recording() == path
    with open(path, encoding="utf-8") as f:
        assert f.read().splitlines() == [HEADER, "0,0,0,0,0.3,-1,-1,-1,-1", "0.5,0.2,0,0,0.9,-1,-1,-1,-1"]
    sim.load(QUAD, open_loop_fc=True)
    sim.play_inputs(path)
    assert sim.run(0.5).motor_cmd == first
    assert sim.run(0.5).motor_cmd == second


def test_the_recording_reports_its_events_and_refuses_misuse(sim):
    sim.load(QUAD, open_loop_fc=True)
    with pytest.raises(ofs.InvalidState):
        sim.stop_inputs_recording()
    path = sim.record_inputs()
    with pytest.raises(ofs.InvalidState):
        sim.record_inputs()
    sim.run(0.1)
    sim.stop_inputs_recording()
    kinds = [(e.kind, e.message) for e in sim.events() if e.kind.startswith("inputs_recording")]
    assert kinds[0] == ("inputs_recording_started", path)
    assert kinds[1][0] == "inputs_recording_stopped" and kinds[1][1].startswith(path)
```
Run: `python -m pytest python/tests/test_inputs_recording.py python/tests/test_client.py -v`
Expected: PASS (needs `cargo build -p ofs-sim` first: the `sim_bin` fixture finds `target/debug/ofs-sim`).

- [ ] **Step 5: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `python -m pytest python/tests -v -k "not sitl"`
Expected: green.

```bash
git add -A proto crates python
git commit -m "feat(proto): protocol 10 - StartInputsRecording and StopInputsRecording

The server names <quad>-inputs-<n>.csv under the logs directory, refuses a second recording and one while an
inputs file plays, reports inputs_recording_started/stopped events, and an unloaded session leaves a complete
file. Python: sim.record_inputs() and sim.stop_inputs_recording().

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The Rust client and the Godot game

**Files:**
- Modify: `crates/ofs-client/src/model.rs`, `crates/ofs-client/src/worker.rs`, `crates/ofs-client/tests/session.rs`
- Modify: `crates/ofs-godot/src/lib.rs`
- Modify: `godot/scripts/app.gd`, `godot/ui/hud.gd`, `godot/tests/test_hud.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 7's RPCs and events.
- Produces: `Command::SetInputsRecording(bool)`, `EventKind::{InputsRecordingStarted, InputsRecordingStopped}` (names `inputs_recording_started`/`_stopped`), `OfsClient.set_inputs_recording(on)`, `app.inputs_recording` (bool), the `I` key, `Hud.sim_text()`.

- [ ] **Step 1: Write the failing client test**

Add to `crates/ofs-client/tests/session.rs`:

```rust
#[test]
fn recording_the_sticks_reports_the_file_and_stops_on_request() {
    let server = TestServer::start();
    let mut probe = flying(&server);
    probe.wait("the pilot link is up", SHORT, |p| p.client.telemetry().is_some_and(|t| t.tx_enabled));
    probe.client.send(Command::SetInputsRecording(true));
    probe.wait("inputs_recording_started", SHORT, |p| p.event_kinds().contains(&EventKind::InputsRecordingStarted));
    probe.client.set_sticks(climb());
    probe.client.send(Command::SetInputsRecording(false));
    probe.wait("inputs_recording_stopped", SHORT, |p| p.event_kinds().contains(&EventKind::InputsRecordingStopped));
}
```
Run: `cargo test -p ofs-client --test session --locked` — expected: FAIL to compile.

- [ ] **Step 2: The client**

`crates/ofs-client/src/model.rs`: add to `EventKind` (before `Unknown`):
```rust
    /// The inputs recording started (the message is its path).
    InputsRecordingStarted,
    /// The inputs recording stopped or failed.
    InputsRecordingStopped,
```
to `EventKind::name`: `EventKind::InputsRecordingStarted => "inputs_recording_started", EventKind::InputsRecordingStopped => "inputs_recording_stopped",`; to `Event::from_pb`: `Ok(pb::EventKind::InputsRecordingStarted) => EventKind::InputsRecordingStarted, Ok(pb::EventKind::InputsRecordingStopped) => EventKind::InputsRecordingStopped,`; to `Command`:
```rust
    /// Starts (true) or stops (false) recording the sticks to a replayable CSV the server names.
    SetInputsRecording(bool),
```
`crates/ofs-client/src/worker.rs`: in the command `match` after `SetRadioLoss`:
```rust
                Some(Command::SetInputsRecording(on)) => {
                    let result = if on {
                        client.start_inputs_recording(pb::Empty {}).await.map(|_| ())
                    } else {
                        client.stop_inputs_recording(pb::Empty {}).await.map(|_| ())
                    };
                    if let Err(status) = result {
                        shared.error(status_error(status));
                    }
                }
```
(if another `match` over `Command` exists, e.g. the failed-state loop near line 394, add the arm there as a no-op like its siblings). `crates/ofs-godot/src/lib.rs`, after `set_radio_loss`:
```rust
    /// Starts (true) or stops (false) recording the sticks to a replayable CSV the server names; the
    /// `inputs_recording_started` / `inputs_recording_stopped` events say where it went.
    #[func]
    fn set_inputs_recording(&self, on: bool) {
        self.send(Command::SetInputsRecording(on));
    }
```
Run: `cargo test -p ofs-client --locked` then `cargo build -p ofs-godot --locked` — expected PASS.

- [ ] **Step 3: Write the failing HUD test, then the game**

Add to `godot/tests/test_hud.gd`:

```gdscript
func test_the_status_line_shows_when_the_sticks_are_being_recorded() -> void:
	var hud := await _hud()
	hud.update_view(_view())
	ok(not hud.sim_text().contains("REC"), "not recording: %s" % hud.sim_text())
	hud.update_view(_view({"recording_inputs": true}))
	ok(hud.sim_text().contains("REC STICKS"), "the indicator: %s" % hud.sim_text())
	hud.update_view(_view({"telemetry": {}, "recording_inputs": true}))
	ok(hud.sim_text().contains("REC STICKS"), "and before the first state: %s" % hud.sim_text())
	hud.queue_free()
```
`godot/ui/hud.gd`: add a help line after the `K` line: `"I  record the sticks to a CSV (replay with play_inputs)",`; add after `battery_text()`:
```gdscript
## The status line (for the tests).
func sim_text() -> String:
	return _sim.text
```
document `recording_inputs` in the `update_view` comment's key list, and in `update_view` after the `if t.is_empty(): ... else: ...` block (before `var sticks`):
```gdscript
	if view.get("recording_inputs", false):
		_sim.text += "   REC STICKS"
```
`godot/scripts/app.gd`: add `var inputs_recording := false` beside the other state vars (near `radio_cut`); add `"recording_inputs": inputs_recording,` to the `hud.update_view({...})` dictionary; in `_unhandled_key_input` add `KEY_I: _toggle_inputs_recording()` after `KEY_K`; add

```gdscript
func _toggle_inputs_recording() -> void:
	if client == null:
		return
	# The state flips when the server's event arrives, so the indicator is never ahead of the file.
	client.set_inputs_recording(not inputs_recording)
```
and to `_on_event`:
```gdscript
		"inputs_recording_started":
			inputs_recording = true
			hud.add_toast("Recording the sticks to %s" % message.get_file())
		"inputs_recording_stopped":
			inputs_recording = false
			hud.add_toast("Sticks recording stopped: %s" % message.get_file())
```
Extend the e2e in `godot/tests/e2e_open_loop.gd` right after the K block (after `_check(restored, "and the HUD says so")`):

```gdscript
	_press(app, KEY_I)
	var recording: bool = await _wait_for(func(): return app.inputs_recording, 10.0)
	_check(recording, "I starts recording the sticks")
	_check(app.hud.sim_text().contains("REC STICKS"), "and the status line says so: %s" % app.hud.sim_text())
	await _wait_for(func(): return false, 0.3)
	_press(app, KEY_I)
	var recorded: bool = await _wait_for(func(): return not app.inputs_recording, 10.0)
	_check(recorded, "I again stops it")
	var logs := data_dir.path_join("logs")
	var csv_files: Array = []
	for f in DirAccess.get_files_at(logs):
		if f.contains("-inputs-") and f.ends_with(".csv"):
			csv_files.append(f)
	_check(csv_files.size() == 1, "one inputs CSV in the logs directory: %s" % str(csv_files))
	if csv_files.size() == 1:
		var text := FileAccess.get_file_as_string(logs.path_join(csv_files[0]))
		_check(text.begins_with("t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4\n"), "it has the play_inputs header: %s" % text.left(80))
		_check(text.split("\n", false).size() >= 2, "and at least the first row")
```
Run: `bash scripts/run-godot-tests.sh all` with `GODOT_BIN` set as dev-setup describes (Windows: `GODOT_BIN="C:/tools/godot/Godot_v4.7.2-stable_win64_console.exe"`).
Expected: unit suites pass and the open-loop e2e passes (the Betaflight e2e skips without SITL).

- [ ] **Step 4: Full check and commit**

Run: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`
Expected: green.

```bash
git add -A crates godot
git commit -m "feat(game): the I key records the sticks to a replayable CSV

OfsClient.set_inputs_recording and Command::SetInputsRecording drive the new RPCs; the status line shows
REC STICKS while the server's inputs_recording_started event stands; the open-loop e2e flies it through the
real key handler and finds the file in the logs directory.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Live checks, the benchmark, the docs, the spec's amendments

**Files:**
- Create: `docs/research/serial-radio.md`
- Modify: `docs/superpowers/specs/2026-10-15-m5-radio-components-design.md` (amended to this plan's rulings)
- Modify: `docs/roadmap.md`, `docs/architecture.md`, `docs/dev-setup.md`, `README.md`, `CHANGELOG.md`, `library/README.md` only if it lists quads

**Interfaces:**
- Consumes: everything above.
- Produces: the merge-ready branch.

- [ ] **Step 1: Run the live and cross-platform checks**

Run each, from the worktree, and record the result in the commit message:

- Windows: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`.
- WSL (Linux, live SITL): `cd /mnt/c/dev/open-fpv-sim/.worktrees/m5-radio-chain && CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf ~/.cargo/bin/cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1` (a separate target dir per checkout; `cargo clean -p ofs-radio -p ofs-sim -p ofs-config -p ofs-client` if the dir is shared with another checkout).
- Python on Windows, with Betaflight in WSL as `docs/dev-setup.md` describes: `cargo build -p ofs-sim`, then `python -m pytest python/tests -v` (the SITL tests need `OFS_SITL_LAUNCH`; if Windows Firewall blocks the WSL binary, run the non-SITL tests here and report the SITL ones as left to CI; never change firewall settings).
- Godot: `bash scripts/run-godot-tests.sh all` (the Betaflight e2e runs when `OFS_SITL_LAUNCH` is set).
- Benchmark: `cargo build -p ofs-sim --release --locked` then `python scripts/benchmark.py --binary target/release/ofs-sim --floor 1`; compare its real-time factor with `main`'s (run it on `main`'s tip in the main checkout, the same machine, the same minute): the chain's dynamic dispatch must not cost more than a few percent.

Expected: all green. Anything that fails is a task defect: fix it in the owning task's files and add the test that would have caught it.

- [ ] **Step 2: The research note**

Create `docs/research/serial-radio.md`:

```markdown
# The handset over USB serial (M5's capture)

Date 2026-10-15. Handset: RadioMaster TX15 (EdgeTX), USB `0483:5740`. Host: Windows 11 and WSL2 Ubuntu.

M5 was first specified as "an ELRS handset in simulator mode speaks CRSF over USB serial; the simulator reads its
channel frames". The capture-first rule (spec §2 of the serial-radio design) tested that premise before building
anything, and it did not hold.

## What was found

- **The port.** In "USB Serial (VCP)" mode the radio enumerates as a composite device with two interfaces. Windows
  binds `usbser` to the first and cannot start it (`CM_PROB_FAILED_START`, status `0xC000000E`); the second
  ("TX15 Serial Port") gets no driver (`CM_PROB_FAILED_INSTALL`). A different cable, port, power cycle, and forcing
  the Microsoft serial driver onto the composite parent changed nothing.
- **From Linux it opens.** With `usbipd-win` sharing the device into WSL (`usbipd bind --busid <id>` once, as
  administrator, then `usbipd attach --wsl Ubuntu --busid <id>`), the WSL kernel's `cdc_acm` creates `/dev/ttyACM0`.
  The device node needs `chmod a+rw` or the `dialout` group.
- **The port is quiet.** At 420000 and 115200 baud, nothing arrived in 4 s with the radio idle. In EdgeTX's
  `USB-VCP` setting (SYS > Hardware: CLI, Lua, Telemetry Mirror) set to CLI the port answers `help`
  (`help [<command>]`): it is a console, not a stream of channel frames. EdgeTX offers no VCP mode that streams the
  radio's CRSF channels; Telemetry Mirror copies telemetry the other way.
- **How simulators read a radio.** Over USB joystick (HID) mode, which the project already supports.

## What it means

- No serial radio in M5: the joystick stays the input, and M5 became swappable radio components and live-flight
  recording (`docs/superpowers/specs/2026-10-15-m5-radio-components-design.md`).
- **Untested routes** (roadmap): the external module bay speaks CRSF (a half-duplex single wire) to whatever is
  plugged in, so a PC with a USB-UART adapter and a bay breakout could play the module; and an EdgeTX Lua script
  could stream channel values over `serialWrite` with USB-VCP in Lua mode (the docs list the functions but not which
  port they use). Neither was tried.
- The Windows driver failure is specific to this radio's descriptors; the Linux path works through `usbipd`.
```

- [ ] **Step 3: Amend the spec to what was built**

In `docs/superpowers/specs/2026-10-15-m5-radio-components-design.md` make these edits (each a plain text change; keep the rest):
- §2 decision 4: "Channels are CRSF 11-bit ticks `[u16; 16]` between the parts; the handset quantizes with today's `stick_ticks`/`throttle_ticks` (byte identity)."
- §2 decision 5 and §3: "`build_chain`" becomes "`radio_chain`, a private function in `ofs-sim/src/vehicle.rs`"; `LinkEnv` becomes "`&Bus` (a link registers the signal handles it reads); the handset's RF front end is read by the builder".
- §3 `Receiver`: the `link_stats_interval_packets` cadence is the receiver's (`ReceiverParams`).
- §5: "`kind` names the link (`elrs` or `fixed`); `handset`, `receiver`, `band` and `[radio.fixed]` are new optional keys; there is no separate `link` key."
- §6: `StopInputsRecording` returns `{path, rows, duration_s}`; the events are `inputs_recording_started` (20) and `inputs_recording_stopped` (21); the Godot toggle is the `I` key plus a `REC STICKS` status-line indicator.
- §4: `rf_mode` for LINK_STATISTICS is the rate's place in the band's ascending list plus one; the 900 MHz figures (915 MHz, 500 kHz, sensitivities 25/50/100/200/250 Hz at -123/-120/-117/-112/-111 dBm) are estimates from ExpressLRS's published table.
- §8/§9: the task outline becomes the plan's nine tasks.

- [ ] **Step 4: The docs**

- `docs/roadmap.md`: replace the line "Real radio input over CRSF serial, and telemetry back to a real radio." with a row: "A real radio over serial: the TX15's USB-VCP does not stream CRSF (`docs/research/serial-radio.md`); the untested routes are the external module bay with a USB-UART adapter, and a Lua script over `serialWrite`. **Blocked on a verified route.**" Add rows: "Crossfire and Tracer links: a profile or a `Link` each now that the seams exist; they need sourced figures." / "Non-CRSF receivers (SBUS, iBUS, FrSky, DSMX): a `Receiver` each, with the matching Betaflight config." / "Plugin loading for radio models from outside the tree." / "The 2.4 GHz 50 Hz sensitivity is -117 dBm in the model; ExpressLRS's table now lists -115 dBm (changing it changes every seeded 50 Hz run)." Under "Radio" add: "The 900 MHz band's carrier, bandwidth, noise figure and downlink power are estimates."
- `docs/architecture.md`: in the `ofs-radio` row and the radio section, describe `Handset`/`Link`/`Receiver`, `RadioChain`, the `elrs` and `fixed` links, and point to `docs/writing-a-radio-model.md`; add the inputs recorder to the `ofs-sim` row.
- `docs/dev-setup.md`: the protocol paragraph (currently "protocol 9") becomes protocol 10 and lists `StartInputsRecording`/`StopInputsRecording`; the manual-check list gains "fly with the joystick, press **I**, fly 20 s, press **I**, then `sim.play_inputs(path)` in a lockstep Python session replays the sticks".
- `README.md`: the keys sentence gains "**I** record the sticks to a replayable CSV"; the quad list (line ~133) gains `opendrone-5f-900mhz.toml` and `opendrone-5f-fixed-link.toml`; one sentence in the radio paragraph: the radio is three swappable parts (`docs/writing-a-radio-model.md`).
- `CHANGELOG.md`: a new `## Unreleased — M5: swappable radio components` section above the close-out one, with **Added** (the three parts and `RadioChain`; the ELRS 900 MHz band and `band`; the `fixed` link and `[radio.fixed]`; `handset`/`receiver` keys; the inputs recorder, protocol 10, `sim.record_inputs()`, the `I` key; the two quads; the golden tests; `docs/writing-a-radio-model.md`; `docs/research/serial-radio.md`) and **Changed** ("`link_stats_interval_packets` moved from the link to the receiver internally; quad files are unchanged").
- Normalize the edited docs to LF (the repo's convention).

- [ ] **Step 5: Commit**

```bash
git add -A docs README.md CHANGELOG.md
git commit -m "docs: M5 - swappable radio components: research note, roadmap, guides, spec as built

Verification: <paste the Step 1 results: Windows test count + clippy, WSL live SITL, Python, Godot, benchmark
factor against main>.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: The maintainer's manual check (not a gate for the merge)**

With the TX15 in USB joystick mode: `godot --path godot`, bind the sticks in F2, fly, press **I**, fly 20 s, press **I**, then in Python `sim.load(quad); sim.play_inputs(<the toast's file>); sim.run(20)` and watch the replay in Rerun or the MCAP in Foxglove. Note the result in `docs/dev-setup.md`'s manual-checks list, as the earlier milestones' checks are.

---

## Self-review

**Spec coverage.** §1 (premise, finding) → Task 9 note. §2 decisions 1-8 → Tasks 2-5; decision 3 (only a link draws) → `parts.rs` docs, `FaultSignals`, tests in Tasks 2/4; decision 6 (characterization first) → Task 1; decision 7 (recording at `set_sticks`) → Task 6; decision 8 (no Crossfire) → Task 9 roadmap. §3 (the parts) → Task 2. §4 (shipped parts) → Tasks 2-4. §5 (config) → Tasks 3-5. §6 (recording, protocol 10, Python, Godot) → Tasks 6-8. §7 (testing 1-7) → Task 1 (1), Task 2 step 11 (2), Tasks 2-4 (3), Task 5 (4), Tasks 3-5 (5), Tasks 6-7 (6), Task 9 step 1 (7). §8 (docs) → Tasks 5, 9.

**Known deviations**, all in "Rulings" above and applied to the spec in Task 9.

**Type consistency.** `RadioChain::new(handset, link, receiver, base_hz, bus)`; `RadioChain::elrs(params, receiver_params, base_hz, seed, uart, telemetry_in, bus)`; `ElrsLink::new(params, seed, bus)`; `FixedLink::new(params, seed, bus)`; `CrsfReceiver::new(params, uart, telemetry_in)`; `JoystickHandset::new(front_end, bus)`; `LinkReport`/`Delivery` fields identical in Tasks 2, 4, 5 tests; `Vehicle::start_inputs_recording(&Path)`, `stop_inputs_recording() -> InputsRecordingSummary {path, rows, duration_s}` identical in Tasks 6-7; event names `inputs_recording_started/stopped` in Tasks 7-8.

**Not verified by running.** This plan was written against the code as read; no step has been compiled. The first implementer expects small compile fixes (imports, a clippy lint or two); the goldens and the replay-digest test are the arbiters of correctness.
