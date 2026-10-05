# M2a — Radio Link, Real-Time Mode and Firmware Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pilot input reaches real Betaflight SITL only through a simulated ExpressLRS link speaking real CRSF bytes. The server runs sessions paced to the wall clock (real time) as well as in lockstep. It streams state and events to clients, survives Betaflight reboots (Configurator "Save"), and fails safe when the radio link or the pilot client goes away.

Exit criteria:
- a radio cut makes Betaflight disarm within its failsafe timing;
- MSP over TCP reports the API version;
- Betaflight Configurator connects to a running real-time session;
- SITL runs stay bit-identical with CRSF.

**Architecture:**
- **Radio.** A new `ofs-radio` crate turns stick signals into CRSF frames at the ELRS packet rate, through a seeded loss model. The frames travel on a bounded byte `Wire` to the SITL bridge. The bridge appends them to the lockstep state datagram, and a small SITL patch extension hands them to UART2 on the tick that carries them. Receiver traffic is therefore deterministic.
- **Server.** The server gains sessions with a run mode, a real-time runner thread driven by a pure `Pacer`, and protocol-2 streams: state, a bidirectional pilot link, and event watchers. A watcher's disconnect ends a session unless `keep_alive` is set; a pilot's disconnect switches the transmitter off.
- **Reboots.** The bridge recognises a Betaflight reboot (exit 0 after `[system]Reset!`) and relaunches SITL from its EEPROM.

**Tech Stack:**
- Rust stable (rust-version 1.85);
- tonic 0.12 / prost 0.13 / tokio 1 (`signal`, `sync`, `time`), proptest 1;
- Python ≥ 3.10 with grpcio ≥ 1.84 and protobuf ≥ 7.35.1;
- Betaflight SITL 2026.6.2 with `third_party/betaflight/ofs-sitl.patch`.

**Spec:** `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md` (M2 row of §9; §1.5, §4.4, §5.1–5.5, §7, §8.2–8.4).
**Read before Tasks 5–9:** `docs/research/sitl-interface.md` (M0 findings).
**Carried debt from M1:** `docs/superpowers/m1-carried-debt.md` (this plan resolves graceful shutdown, Run cancellation on disconnect, and firmware directories keyed only on the file stem).

## Scope: M2 is split in two plans

Spec §9 defines M2 as: "Fly with a joystick in Godot (grey-box world, FPV camera) through the ELRS/CRSF stack; failsafe works; Configurator connects."
- **This plan (M2a)** delivers everything headless: the radio stack, real-time mode, the protocol-2 API the Godot client needs, failsafe, and Configurator connectivity. Every part is testable in CI without a GPU.
- **M2b (next plan)** delivers the Godot client: the godot-rust GDExtension acting as a gRPC client of `Pilot`/`StreamState`/`Watch`, the grey-box world, the FPV camera, joystick capture, the HUD, and the headless Godot smoke test.

## Decisions made while planning (verified in a scratch build before writing this plan)

1. **CRSF bytes ride inside the lockstep state datagram.** Bytes on SITL's TCP UART arrive asynchronously to the tick, so runs would not be reproducible (M0 §5 caveat). Instead, blocks of `[uart index][len u16 LE][bytes]` follow the 184-byte datagram. The patched SITL stages them with the packet and hands them to the UART on the tick that applies it.
   - Verified: Betaflight reads CRSF sticks 0.2 / −0.2 / 0 / throttle 0 as exactly 1600 / 1400 / 1500 / 1000 µs over MSP_RC.
   - Real tools can still use the TCP UARTs.
2. **The UDP `rc_packet` channels are sent as zeros.** Betaflight always sees a CRSF receiver (spec §1.5). Zeros are invalid pulses, so a SITL whose EEPROM still selects the UDP receiver fails safe instead of silently flying.
3. **Deterministic boot.** Making CRSF runs reproducible exposed a boot race: SITL's UDP thread starts early in `systemInit()`, so the first state packet could land while init still ran. Init then read the fake sensors before or after the packet's values arrived; the Blackbox showed the attitude estimate differing by 1–2 counts at arming, and motors differing by one PWM step 2 s later.
   - Fix: SITL ignores state packets until its scheduler runs, announces `[SITL] ready for the simulator`, and runs no task before the first packet. The bridge waits for that line instead of probing TCP 5761 and sleeping 300 ms.
   - Verified: 8/8 compared flights bit-identical, against about 40 % diverging without the fix.
4. **Bytes between models travel on a bounded `Wire`** (`ofs_core::Wire`), not on the f64/vec/quat signal bus. When nothing drains it (open-loop FC), the oldest bytes are dropped like a UART overrun.
5. **Real-time pacing has two policies (spec §5.1).**
   - `warn` catches up in bursts while behind by ≤ 100 ms, drops a larger backlog and counts an overrun.
   - `slow` never bursts more than one 50 ms chunk and counts one overrun per 100 ms of accumulated stretch.
   - In both, the simulation itself stays exact.
6. **Session lifetime (spec §7).**
   - A script client's session ends when its last `Watch` stream closes, unless it was loaded with `keep_alive`.
   - A pilot's `Pilot` stream closing switches the transmitter off, so Betaflight fails safe while the session keeps running for a reconnect.
   - The Python client opens a `Watch` stream on its first `load`.
7. **A Betaflight reboot is a firmware restart, not a crash.** It is recognised as a clean exit after the `[system]Reset` line, waiting up to 3 s, because `motorShutdown()` sleeps 0.5 s before resetting. The bridge relaunches SITL from its EEPROM and counts `fc.restarts`.
8. **Firmware directories are `<data_dir>/<quad stem>-<8 hex of the quad path hash>`.** A quad whose `betaflight.diff` changed since its first boot is refused with instructions, instead of flying a stale EEPROM.
9. **Protocol version 2, quad schema version 2.** The `[radio]` section is required; old quad files fail with the explicit schema message.

## Global Constraints

- **License:** GPL-3.0-or-later (code). Every crate's `Cargo.toml` uses `license.workspace = true`.
- **Platforms:** Windows and Linux. Rust code must build and test on both. SITL runs natively on Linux and under WSL2 on Windows.
- **Toolchain:** Rust stable, `rust-version = "1.85"` (workspace). Python ≥ 3.10.
- **Layout:** `crates/` (Rust workspace), `python/`, `quads/`, `proto/`, `docs/`, `third_party/`. Crate prefix `ofs-`.
- **Frames and units:** world NED, body FRD, quaternions body→world. SI units; signal and field names carry a unit suffix.
- **Rates:** the base tick is 8 kHz, and every model's rate is an integer divisor of it. ELRS packet rates must divide `sim.base_hz`.
- **Determinism:** same quad + seed + inputs in lockstep ⇒ identical bus digest, Betaflight SITL included (patched build, no first-datagram resend). All randomness comes from `ofs_core::rng::model_rng(seed, model_name)`.
- **Errors:** simulator failures (`SimError`) stop the run loudly and are never presented as drone behaviour. Simulated failures (radio loss, failsafe) are behaviour. `SimError` variants stay `Firmware`, `NonFinite`, `InvalidArgument`, `Other`.
- **Pilot input** reaches Betaflight only as CRSF on the receiver UART (quad `radio.uart`, default UART2).
- **SITL protocol** (M0, extended here):
  - one datagram per exchange to UDP 9003: 184 bytes (`fdm_packet` ‖ `rc_packet`), then optional UART blocks of at most 512 bytes in total;
  - one 16-byte `servo_packet` back on UDP 9002;
  - never send to 9004;
  - exchange rate 1000 Hz;
  - first reply waits up to 5 s with resends every 250 ms, then 500 ms per reply.
- **One SITL per machine** (fixed ports). Live SITL tests run with `--test-threads=1`.
- **gRPC:** `ofs-error-kind` metadata values are `config`, `firmware`, `numerical`, `protocol`, `not_loaded`, `invalid_argument`, `invalid_state`, `pilot_busy`, `internal`. Client state streams run at 1–240 Hz.
- **Not in M2a:**
  - the Godot client (M2b);
  - CRSF telemetry from SITL;
  - VTX/OSD/SmartAudio (M3);
  - MCAP/Rerun and the fault catalog beyond radio link loss (M4);
  - a radio path-loss model;
  - playing recorded input files (`play_inputs`, M4 scripting);
  - real radio input over serial.

## Review Focus

- **An existing firmware directory from M1** (UDP receiver in `eeprom.bin`). This must not silently fly or silently fail to arm: the new directory naming gives the quad a fresh EEPROM, and a changed diff is refused with instructions. Test: Task 7 `a_changed_quad_diff_refuses_a_stale_eeprom`.
- **Betaflight Configurator "Save"** (reboot mid-session). The session continues on a relaunched SITL; this is not a firmware crash. Test: Task 9 `a_betaflight_reboot_is_a_firmware_restart_not_a_crash`.
- **The pilot client vanishing** (Godot crash, network drop). The transmitter goes off, Betaflight fails safe, and the session keeps running for a reconnect. Test: Task 12 `a_vanished_pilot_switches_the_transmitter_off`.
- **A script killed mid-`Run`.** The server stops stepping within one 50 ms chunk and releases the session; without `keep_alive` the session ends. Tests: Task 11 `an_abandoned_run_stops_and_releases_the_session`, Task 12 `the_last_watcher_leaving_ends_a_session_unless_keep_alive`, Task 13 `test_closing_a_client_ends_its_session`.
- **Real time on a slow host** (Windows + WSL, coarse sleeps). Overruns are counted and catch-up never runs away. Tests: Task 10 `warn_drops_a_backlog_beyond_the_allowed_lag_and_counts_an_overrun`, `slow_never_bursts_and_counts_an_overrun_per_100_ms_of_stretch`.
- **Malformed bytes into any parser.** They never panic. Tests: Task 2 `decoder_never_panics_and_holds_at_most_one_frame` (proptest), Task 4 `parser_skips_noise_and_bad_checksums`.

## Known intermittents to watch while executing

The final verification in the scratch build hit two intermittents:
- **Reboot test, full suite.** The live reboot test failed once in about 16 runs, and only in a full-suite run.
- **First live run after a SITL rebuild.** The very first live run after rebuilding SITL timed out on its first exchange, in both tests of that binary; the next 6 rounds were clean.

Neither reproduced on demand. Tasks 5, 8 and 9 each repeat their live tests. On any failure, capture the full `FirmwareCrashed`/`Firmware` message (it includes SITL's last output) and debug with superpowers:systematic-debugging before completing the task. Do not retry until it passes.

## File Structure

```
Cargo.toml                                    + ofs-radio, proptest; tokio signal/sync/time
crates/ofs-core/src/{lib,names,wire}.rs       Wire byte port; radio, fault and FC-restart signal names
crates/ofs-radio/                             NEW crate: crsf.rs (CRSF codec), elrs.rs (ExpressLRS link model)
crates/ofs-fc/src/msp.rs                      NEW: MSP v1 client over TCP
crates/ofs-fc/src/sitl/{codec,bridge,process}.rs   UART blocks in the datagram, ready line, diff check, reboot restart
crates/ofs-fc/tests/{msp,sitl_codec,sitl_errors,sitl_live}.rs, tests/data/crsf.betaflight.diff
third_party/betaflight/ofs-sitl.patch         + UART bytes in the datagram, deterministic boot
third_party/betaflight/tools/{add_serial_in_datagram,deterministic_boot}.py   generators for those patch changes
crates/ofs-config/src/lib.rs, tests/config.rs [radio] section, schema 2
quads/opendrone-5f-freestyle.{toml,betaflight.diff}  radio section; CRSF receiver on UART2
crates/ofs-sim/src/{vehicle,pacer,session,runner,streams,server,main,lib}.rs
crates/ofs-sim/tests/{open_loop_vehicle,pacer,grpc,sitl_live}.rs
proto/ofs/v1/sim.proto                        protocol 2
python/ofs/{client,errors,faults,__init__}.py, python/ofs/v1/ (regenerated)
python/tests/{test_realtime,test_sitl_radio}.py (new), test_sitl_hover.py; python/examples/serve_realtime.py (new)
docs/dev-setup.md, README.md, docs/research/sitl-interface.md, docs/superpowers/m1-carried-debt.md, .github/workflows/ci.yml
```

**Environment notes for every task:**
- **Windows git bash:** run `export PATH="$HOME/.cargo/bin:$PATH"` first.
- **Linux tests (WSL):**

  ```bash
  wsl.exe -d Ubuntu -e bash -lc 'cd /mnt/c/dev/open-fpv-sim && CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test --workspace --locked'
  ```

- **Live SITL from Windows:** `OFS_SITL_LAUNCH="wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf"`.
- **Cleanup check:** after any live run, `wsl.exe -d Ubuntu -e pgrep -x betaflight_SITL` must print nothing.

---

### Task 1: Byte wires and new signal names in `ofs-core`

**Files:**
- Create: `crates/ofs-core/src/wire.rs`, `crates/ofs-core/tests/wire.rs`
- Modify: `crates/ofs-core/src/lib.rs`, `crates/ofs-core/src/names.rs` (append)

**Interfaces:**
- Produces:
  - `ofs_core::Wire` (`Wire::new(capacity: usize)`, `write(&self, &[u8])`, `take(&self, max: usize) -> Vec<u8>`, `len`, `is_empty`, `dropped(&self) -> u64`; `Clone` shares one FIFO; `Send + Sync`);
  - `names::{RADIO_TX_ENABLED, RADIO_LINK_UP, RADIO_LQ, RADIO_RSSI, FAULT_RADIO_LINK_LOSS, FC_RESTARTS}`.

- [ ] **Step 1: Write the failing test** `crates/ofs-core/tests/wire.rs`:

```rust
use ofs_core::Wire;

#[test]
fn bytes_come_out_in_order() {
    let w = Wire::new(16);
    w.write(&[1, 2, 3]);
    w.write(&[4]);
    assert_eq!(w.take(2), vec![1, 2]);
    assert_eq!(w.take(10), vec![3, 4]);
    assert!(w.is_empty());
}

#[test]
fn clones_share_one_fifo() {
    let a = Wire::new(16);
    let b = a.clone();
    a.write(&[7, 8]);
    assert_eq!(b.len(), 2);
    assert_eq!(b.take(usize::MAX), vec![7, 8]);
    assert!(a.is_empty());
}

#[test]
fn overflow_drops_the_oldest_bytes() {
    let w = Wire::new(4);
    w.write(&[1, 2, 3]);
    w.write(&[4, 5, 6]);
    assert_eq!(w.take(usize::MAX), vec![3, 4, 5, 6]);
    assert_eq!(w.dropped(), 2);
}

#[test]
#[should_panic(expected = "capacity")]
fn zero_capacity_is_rejected() {
    Wire::new(0);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p ofs-core --test wire`
Expected: compile error, no `Wire` in `ofs_core`.

- [ ] **Step 3: Implement** `crates/ofs-core/src/wire.rs`:

```rust
//! Byte ports between models: a serial line from one model's TX to another's RX (for example a radio
//! receiver's CRSF output into the flight controller's UART). Bounded like a UART RX buffer: when full,
//! the oldest bytes are dropped and counted. The scheduler's fixed model order keeps traffic deterministic.
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug)]
struct Fifo {
    bytes: VecDeque<u8>,
    capacity: usize,
    dropped: u64,
}

/// A shared, bounded byte FIFO. Clones are handles to the same FIFO.
#[derive(Debug, Clone)]
pub struct Wire(Arc<Mutex<Fifo>>);

impl Wire {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "wire capacity must be > 0");
        Self(Arc::new(Mutex::new(Fifo { bytes: VecDeque::new(), capacity, dropped: 0 })))
    }

    fn fifo(&self) -> MutexGuard<'_, Fifo> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Appends `bytes`; if the FIFO overflows, the oldest bytes are discarded.
    pub fn write(&self, bytes: &[u8]) {
        let mut f = self.fifo();
        f.bytes.extend(bytes);
        let excess = f.bytes.len().saturating_sub(f.capacity);
        if excess > 0 {
            f.bytes.drain(..excess);
            f.dropped += excess as u64;
        }
    }

    /// Removes and returns up to `max` bytes, oldest first.
    pub fn take(&self, max: usize) -> Vec<u8> {
        let mut f = self.fifo();
        let n = f.bytes.len().min(max);
        f.bytes.drain(..n).collect()
    }

    pub fn len(&self) -> usize {
        self.fifo().bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes discarded because the FIFO was full.
    pub fn dropped(&self) -> u64 {
        self.fifo().dropped
    }
}
```

Replace `crates/ofs-core/src/lib.rs` with:

```rust
//! Open FPV Sim core: typed signal bus, models and scheduler. Deterministic, no I/O.
pub mod bus;
pub mod consts;
pub mod interp;
pub mod model;
pub mod names;
pub mod rng;
pub mod scheduler;
pub mod wire;

pub use bus::{Bus, BusValue, Signal, SignalKind};
pub use model::{Model, SimError, StepCtx};
pub use scheduler::Scheduler;
pub use wire::Wire;
```

Append to `crates/ofs-core/src/names.rs`:

```rust

/// Transmitter (handset) power: 1 = on, 0 = off (no pilot connected). Read by the radio link.
pub const RADIO_TX_ENABLED: &str = "radio.tx_enabled";
/// 1 while the receiver has heard at least one packet in its link-quality window, else 0.
pub const RADIO_LINK_UP: &str = "radio.link_up";
/// Uplink link quality: percent of the last 100 packets received.
pub const RADIO_LQ: &str = "radio.lq_pct";
/// Uplink RSSI as the receiver reports it.
pub const RADIO_RSSI: &str = "radio.rssi_dbm";
/// Fault: 1 = every uplink packet is lost.
pub const FAULT_RADIO_LINK_LOSS: &str = "fault.radio.link_loss";
/// Number of firmware restarts so far (e.g. Betaflight rebooting after a Configurator save).
pub const FC_RESTARTS: &str = "fc.restarts";
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-core`
Expected: 8 bus + 4 scheduler + 4 wire tests pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-core
git commit -m "feat(core): bounded byte wires and radio/fault signal names"
```

---

### Task 2: `ofs-radio` crate — CRSF codec

**Files:**
- Modify: `Cargo.toml` (workspace dependencies)
- Create: `crates/ofs-radio/Cargo.toml`, `crates/ofs-radio/src/lib.rs`, `crates/ofs-radio/src/crsf.rs`, `crates/ofs-radio/tests/crsf.rs`

**Interfaces:**
- Produces, in `ofs_radio::crsf`:
  - `crc8_dvb_s2(&[u8]) -> u8`;
  - `stick_ticks(f64) -> u16`, `throttle_ticks(f64) -> u16`, `ticks_to_us(u16) -> u16`;
  - `pack_channels(&[u16; 16]) -> [u8; 22]`, `unpack_channels`;
  - `rc_channels_frame(&[u16; 16]) -> Vec<u8>`;
  - `LinkStatistics` (10-byte payload; `encode`/`decode`) and `link_statistics_frame(&LinkStatistics) -> Vec<u8>`;
  - `enum Frame { RcChannels([u16;16]), LinkStatistics(LinkStatistics), Other{..} }`;
  - `Decoder::default().push(&[u8]) -> Vec<Frame>`, `crc_errors()`, `buffered()`;
  - constants `ADDRESS_FLIGHT_CONTROLLER = 0xC8`, `FRAMETYPE_RC_CHANNELS_PACKED = 0x16`, `FRAMETYPE_LINK_STATISTICS = 0x14`, `CHANNEL_COUNT = 16`, `FRAME_SIZE_MAX = 64`.
- The tick scale is the one M0 verified against SITL: 1000..2000 µs ↔ 192..1792 ticks, 1500 µs = 992. Betaflight converts back with `0.62477·ticks + 881`.

- [ ] **Step 1: Add the workspace dependencies.** In the root `Cargo.toml`, under `[workspace.dependencies]`, add after `tempfile = "3"`:

```toml
proptest = "1"
```

and after `ofs-fc = { path = "crates/ofs-fc" }`:

```toml
ofs-radio = { path = "crates/ofs-radio" }
```

Create `crates/ofs-radio/Cargo.toml`:

```toml
[package]
name = "ofs-radio"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
rand.workspace = true
rand_chacha.workspace = true

[dev-dependencies]
proptest.workspace = true
```

Create `crates/ofs-radio/src/lib.rs`:

```rust
//! Radio link models: CRSF framing and the ExpressLRS link.
pub mod crsf;
```

- [ ] **Step 2: Write the failing tests** `crates/ofs-radio/tests/crsf.rs`:

```rust
use ofs_radio::crsf::*;
use proptest::prelude::*;

#[test]
fn crc_matches_the_crc8_dvb_s2_check_value() {
    assert_eq!(crc8_dvb_s2(b"123456789"), 0xBC);
}

#[test]
fn channels_pack_11_bits_each_lowest_bits_first() {
    let mut ch = [0u16; CHANNEL_COUNT];
    ch[0] = 0x7FF;
    assert_eq!(&pack_channels(&ch)[..3], &[0xFF, 0x07, 0x00]);
    let mut ch = [0u16; CHANNEL_COUNT];
    ch[1] = 0x7FF;
    let p = pack_channels(&ch);
    assert_eq!(&p[..3], &[0x00, 0xF8, 0x3F]);
    assert!(p[3..].iter().all(|b| *b == 0));
}

#[test]
fn stick_mapping_matches_betaflight_microseconds() {
    assert_eq!(stick_ticks(0.0), 992);
    assert_eq!(stick_ticks(-1.0), 192);
    assert_eq!(stick_ticks(1.0), 1792);
    assert_eq!(stick_ticks(5.0), 1792, "clamped");
    assert_eq!(throttle_ticks(0.0), 192);
    assert_eq!(throttle_ticks(1.0), 1792);
    // What M0 read back through SITL's MSP_RC for CRSF input (docs/research/sitl-interface.md §5).
    assert_eq!(ticks_to_us(stick_ticks(0.2)), 1600);
    assert_eq!(ticks_to_us(stick_ticks(-0.2)), 1400);
    assert_eq!(ticks_to_us(stick_ticks(0.0)), 1500);
    assert_eq!(ticks_to_us(throttle_ticks(0.0)), 1000);
}

#[test]
fn rc_frame_layout() {
    let f = rc_channels_frame(&[992; CHANNEL_COUNT]);
    assert_eq!(f.len(), 26);
    assert_eq!(&f[..3], &[ADDRESS_FLIGHT_CONTROLLER, 24, FRAMETYPE_RC_CHANNELS_PACKED]);
    assert_eq!(f[25], crc8_dvb_s2(&f[2..25]));
}

#[test]
fn decoder_round_trips_rc_and_link_statistics() {
    let ch: [u16; CHANNEL_COUNT] = std::array::from_fn(|i| 172 + 100 * i as u16);
    let stats = LinkStatistics { uplink_rssi_1: 60, uplink_rssi_2: 61, uplink_lq: 100, uplink_snr: -5, rf_mode: 7, ..Default::default() };
    let mut bytes = rc_channels_frame(&ch);
    bytes.extend(link_statistics_frame(&stats));
    assert_eq!(Decoder::default().push(&bytes), vec![Frame::RcChannels(ch), Frame::LinkStatistics(stats)]);
}

#[test]
fn decoder_resynchronises_after_garbage_and_split_input() {
    let mut input = vec![0x00, 0xFF, 0xC8]; // noise, including a stray sync byte
    input.extend(rc_channels_frame(&[992; CHANNEL_COUNT]));
    let (a, b) = input.split_at(10);
    let mut d = Decoder::default();
    assert!(d.push(a).is_empty());
    assert_eq!(d.push(b), vec![Frame::RcChannels([992; CHANNEL_COUNT])]);
}

#[test]
fn corrupt_crc_is_counted_and_skipped() {
    let mut input = rc_channels_frame(&[0; CHANNEL_COUNT]);
    *input.last_mut().unwrap() ^= 0xFF;
    input.extend(rc_channels_frame(&[1000; CHANNEL_COUNT]));
    let mut d = Decoder::default();
    assert_eq!(d.push(&input), vec![Frame::RcChannels([1000; CHANNEL_COUNT])]);
    assert_eq!(d.crc_errors(), 1);
}

proptest! {
    #[test]
    fn decoder_never_panics_and_holds_at_most_one_frame(
        chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..80), 0..20)
    ) {
        let mut d = Decoder::default();
        for c in &chunks {
            let _ = d.push(c);
            prop_assert!(d.buffered() < FRAME_SIZE_MAX);
        }
    }

    #[test]
    fn any_channels_round_trip(ch in proptest::array::uniform16(0u16..2048)) {
        prop_assert_eq!(Decoder::default().push(&rc_channels_frame(&ch)), vec![Frame::RcChannels(ch)]);
    }
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p ofs-radio`
Expected: compile errors, `crsf` is empty.

- [ ] **Step 4: Implement** `crates/ofs-radio/src/crsf.rs`:

```rust
//! CRSF framing, byte-compatible with Betaflight's serial RX (src/main/rx/crsf.c, crsf_protocol.h):
//! `[address][length][type][payload...][crc]`, where length counts type + payload + CRC and the CRC is
//! CRC-8/DVB-S2 over type + payload.

pub const ADDRESS_FLIGHT_CONTROLLER: u8 = 0xC8;
/// Addresses a frame can start with (flight controller, radio transmitter, receiver, TX module).
pub const SYNC_ADDRESSES: [u8; 4] = [0xC8, 0xEA, 0xEC, 0xEE];
pub const FRAMETYPE_LINK_STATISTICS: u8 = 0x14;
pub const FRAMETYPE_RC_CHANNELS_PACKED: u8 = 0x16;
pub const CHANNEL_COUNT: usize = 16;
pub const RC_CHANNELS_PAYLOAD_SIZE: usize = 22;
pub const LINK_STATISTICS_PAYLOAD_SIZE: usize = 10;
/// Whole frame including the address and length bytes (Betaflight's CRSF_FRAME_SIZE_MAX).
pub const FRAME_SIZE_MAX: usize = 64;

/// CRC-8/DVB-S2: polynomial 0xD5, initial value 0.
pub fn crc8_dvb_s2(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0xD5 } else { crc << 1 };
        }
    }
    crc
}

/// Stick position in [-1, 1] to CRSF ticks: 1000..2000 us maps to 192..1792 ticks (1500 us = 992).
/// Betaflight turns ticks back into microseconds with us = 0.62477 * ticks + 881 (crsfReadRawRC);
/// this scale was verified against SITL in M0 (docs/research/sitl-interface.md §5).
pub fn stick_ticks(x: f64) -> u16 {
    (992.0 + 800.0 * x.clamp(-1.0, 1.0)).round() as u16
}

/// Throttle in [0, 1] to CRSF ticks (0 = 1000 us = 192 ticks, 1 = 2000 us = 1792 ticks).
pub fn throttle_ticks(t: f64) -> u16 {
    (192.0 + 1600.0 * t.clamp(0.0, 1.0)).round() as u16
}

/// Betaflight's conversion back to microseconds, truncated as MSP_RC reports it.
pub fn ticks_to_us(ticks: u16) -> u16 {
    (0.624_771_201_952_41 * f64::from(ticks) + 881.0) as u16
}

/// Packs 16 channels of 11 bits each, little-endian bit order (channel 0 in the lowest bits).
pub fn pack_channels(channels: &[u16; CHANNEL_COUNT]) -> [u8; RC_CHANNELS_PAYLOAD_SIZE] {
    let mut out = [0u8; RC_CHANNELS_PAYLOAD_SIZE];
    for (i, &ch) in channels.iter().enumerate() {
        let value = ch & 0x7FF;
        for bit in 0..11 {
            if (value >> bit) & 1 == 1 {
                let pos = i * 11 + bit;
                out[pos / 8] |= 1u8 << (pos % 8);
            }
        }
    }
    out
}

pub fn unpack_channels(payload: &[u8; RC_CHANNELS_PAYLOAD_SIZE]) -> [u16; CHANNEL_COUNT] {
    let mut out = [0u16; CHANNEL_COUNT];
    for (i, ch) in out.iter_mut().enumerate() {
        for bit in 0..11 {
            let pos = i * 11 + bit;
            if (payload[pos / 8] >> (pos % 8)) & 1 == 1 {
                *ch |= 1u16 << bit;
            }
        }
    }
    out
}

fn frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 4);
    f.push(ADDRESS_FLIGHT_CONTROLLER);
    f.push(u8::try_from(payload.len() + 2).expect("CRSF payload too long"));
    f.push(frame_type);
    f.extend_from_slice(payload);
    let crc = crc8_dvb_s2(&f[2..]);
    f.push(crc);
    f
}

pub fn rc_channels_frame(channels: &[u16; CHANNEL_COUNT]) -> Vec<u8> {
    frame(FRAMETYPE_RC_CHANNELS_PACKED, &pack_channels(channels))
}

/// LINK_STATISTICS payload (Betaflight's crsfLinkStatistics_t). RSSI fields hold -dBm (60 = -60 dBm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkStatistics {
    pub uplink_rssi_1: u8,
    pub uplink_rssi_2: u8,
    pub uplink_lq: u8,
    pub uplink_snr: i8,
    pub active_antenna: u8,
    pub rf_mode: u8,
    pub uplink_tx_power: u8,
    pub downlink_rssi: u8,
    pub downlink_lq: u8,
    pub downlink_snr: i8,
}

impl LinkStatistics {
    pub fn encode(&self) -> [u8; LINK_STATISTICS_PAYLOAD_SIZE] {
        [
            self.uplink_rssi_1,
            self.uplink_rssi_2,
            self.uplink_lq,
            self.uplink_snr as u8,
            self.active_antenna,
            self.rf_mode,
            self.uplink_tx_power,
            self.downlink_rssi,
            self.downlink_lq,
            self.downlink_snr as u8,
        ]
    }

    pub fn decode(p: &[u8; LINK_STATISTICS_PAYLOAD_SIZE]) -> Self {
        Self {
            uplink_rssi_1: p[0],
            uplink_rssi_2: p[1],
            uplink_lq: p[2],
            uplink_snr: p[3] as i8,
            active_antenna: p[4],
            rf_mode: p[5],
            uplink_tx_power: p[6],
            downlink_rssi: p[7],
            downlink_lq: p[8],
            downlink_snr: p[9] as i8,
        }
    }
}

pub fn link_statistics_frame(stats: &LinkStatistics) -> Vec<u8> {
    frame(FRAMETYPE_LINK_STATISTICS, &stats.encode())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    RcChannels([u16; CHANNEL_COUNT]),
    LinkStatistics(LinkStatistics),
    Other { address: u8, frame_type: u8, payload: Vec<u8> },
}

/// Streaming CRSF parser: feed it bytes in any chunking. It skips to the next sync address after a bad
/// length or CRC, never buffers more than one frame, and no input makes it panic.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    crc_errors: u64,
}

impl Decoder {
    pub fn crc_errors(&self) -> u64 {
        self.crc_errors
    }

    /// Bytes held while waiting for the rest of a frame.
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            match self.buf.iter().position(|b| SYNC_ADDRESSES.contains(b)) {
                Some(start) => {
                    self.buf.drain(..start);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            if self.buf.len() < 2 {
                break;
            }
            let len = usize::from(self.buf[1]); // type + payload + CRC
            if !(2..=FRAME_SIZE_MAX - 2).contains(&len) {
                self.buf.remove(0);
                continue;
            }
            if self.buf.len() < len + 2 {
                break;
            }
            let body = &self.buf[2..len + 1];
            if crc8_dvb_s2(body) != self.buf[len + 1] {
                self.crc_errors += 1;
                self.buf.remove(0);
                continue;
            }
            frames.push(parse(self.buf[0], body[0], &body[1..]));
            self.buf.drain(..len + 2);
        }
        frames
    }
}

fn parse(address: u8, frame_type: u8, payload: &[u8]) -> Frame {
    match frame_type {
        FRAMETYPE_RC_CHANNELS_PACKED => {
            if let Ok(p) = <&[u8; RC_CHANNELS_PAYLOAD_SIZE]>::try_from(payload) {
                return Frame::RcChannels(unpack_channels(p));
            }
        }
        FRAMETYPE_LINK_STATISTICS => {
            if let Ok(p) = <&[u8; LINK_STATISTICS_PAYLOAD_SIZE]>::try_from(payload) {
                return Frame::LinkStatistics(LinkStatistics::decode(p));
            }
        }
        _ => {}
    }
    Frame::Other { address, frame_type, payload: payload.to_vec() }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ofs-radio`
Expected: 9 tests pass (7 unit tests and 2 proptest properties), no warnings. `Cargo.lock` gains proptest and its dependencies; commit it with the crate.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/ofs-radio
git commit -m "feat(radio): CRSF framing compatible with Betaflight's serial RX"
```

---

### Task 3: ExpressLRS link model

**Files:**
- Create: `crates/ofs-radio/src/elrs.rs`, `crates/ofs-radio/tests/elrs.rs`
- Modify: `crates/ofs-radio/src/lib.rs`

**Interfaces:**
- Consumes:
  - `ofs_core::{Wire, Model, Bus, names::*}` (Task 1);
  - `crsf::{rc_channels_frame, link_statistics_frame, stick_ticks, throttle_ticks, LinkStatistics, CHANNEL_COUNT}` (Task 2).
- Produces:
  - `ofs_radio::elrs::{ElrsLink, LinkParams, MODEL_NAME = "radio.elrs", LQ_WINDOW = 100, NO_SIGNAL_RSSI_DBM = -130.0}`;
  - `ElrsLink::new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus)`;
  - `LinkParams { packet_rate_hz, latency_packets, loss_good, loss_bad, p_good_to_bad, p_bad_to_good, rssi_dbm, snr_db, link_stats_interval_packets, rf_mode: u8, tx_power: u8 }` and `LinkParams::ideal(packet_rate_hz)`.
- Bus inputs: `rc.roll/pitch/yaw/throttle`, `rc.aux.0..3`, `radio.tx_enabled`, `fault.radio.link_loss`.
- Bus outputs: `radio.link_up` (1/0), `radio.lq_pct`, `radio.rssi_dbm`.
- Channel order: AETR then AUX1..4; channels 8..15 centred (992).

- [ ] **Step 1: Write the failing tests** `crates/ofs-radio/tests/elrs.rs`:

```rust
use ofs_core::{names, Bus, Scheduler, Wire};
use ofs_radio::crsf::{Decoder, Frame};
use ofs_radio::elrs::{ElrsLink, LinkParams, NO_SIGNAL_RSSI_DBM};

const BASE_HZ: u32 = 8000;

fn rig(params: LinkParams, seed: u64) -> (Scheduler, Wire) {
    let mut bus = Bus::new();
    let uart = Wire::new(1 << 20);
    let div = BASE_HZ / params.packet_rate_hz;
    let link = ElrsLink::new(params, div, seed, uart.clone(), &mut bus);
    let tx = bus.signal::<f64>(names::RADIO_TX_ENABLED);
    bus.set(tx, 1.0);
    let mut s = Scheduler::new(BASE_HZ, bus);
    s.add(Box::new(link));
    (s, uart)
}

fn set(s: &mut Scheduler, name: &str, v: f64) {
    let sig = s.bus_mut().signal::<f64>(name);
    s.bus_mut().set(sig, v);
}

fn get(s: &Scheduler, name: &str) -> f64 {
    s.bus().get(s.bus().lookup::<f64>(name).unwrap())
}

fn frames(uart: &Wire) -> Vec<Frame> {
    Decoder::default().push(&uart.take(usize::MAX))
}

fn rc(frames: &[Frame]) -> Vec<[u16; 16]> {
    frames.iter().filter_map(|f| if let Frame::RcChannels(c) = f { Some(*c) } else { None }).collect()
}

/// Steps one radio packet (16 base ticks at 500 Hz).
fn packet(s: &mut Scheduler) {
    for _ in 0..BASE_HZ / 500 {
        s.step().unwrap();
    }
}

#[test]
fn ideal_link_sends_one_rc_frame_per_packet_and_periodic_statistics() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(1.0).unwrap();
    let f = frames(&uart);
    assert_eq!(rc(&f).len(), 500);
    assert_eq!(f.iter().filter(|f| matches!(f, Frame::LinkStatistics(_))).count(), 10);
    assert_eq!(get(&s, names::RADIO_LQ), 100.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    assert_eq!(get(&s, names::RADIO_RSSI), -50.0);
    let Some(Frame::LinkStatistics(stats)) = f.iter().find(|f| matches!(f, Frame::LinkStatistics(_))) else { unreachable!() };
    assert_eq!((stats.uplink_rssi_1, stats.uplink_lq, stats.uplink_snr), (50, 100, 10));
}

#[test]
fn sticks_become_crsf_channels_in_aetr_order() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    for (name, v) in [(names::RC_ROLL, 0.2), (names::RC_PITCH, -0.2), (names::RC_YAW, 0.5), (names::RC_THROTTLE, 0.25)] {
        set(&mut s, name, v);
    }
    set(&mut s, &names::rc_aux(0), 1.0);
    set(&mut s, &names::rc_aux(1), -1.0);
    s.run_for(0.01).unwrap();
    let last = *rc(&frames(&uart)).last().unwrap();
    assert_eq!(&last[..6], &[1152, 832, 592, 1392, 1792, 192]);
    assert!(last[8..].iter().all(|c| *c == 992), "unused channels are centred: {last:?}");
}

#[test]
fn transmitter_off_silences_the_receiver() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    set(&mut s, names::RADIO_TX_ENABLED, 0.0);
    s.run_for(0.5).unwrap();
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LQ), 0.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    assert_eq!(get(&s, names::RADIO_RSSI), NO_SIGNAL_RSSI_DBM);
}

#[test]
fn link_loss_fault_drops_and_restores_the_link() {
    let (mut s, uart) = rig(LinkParams::ideal(500), 1);
    s.run_for(0.5).unwrap();
    uart.take(usize::MAX);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 1.0);
    s.run_for(0.1).unwrap(); // 50 packets: half the LQ window
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LQ), 50.0);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
    s.run_for(0.1).unwrap(); // the whole window is lost now
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
    set(&mut s, names::FAULT_RADIO_LINK_LOSS, 0.0);
    s.run_for(0.1).unwrap();
    assert_eq!(rc(&frames(&uart)).len(), 50);
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0);
}

#[test]
fn random_loss_lowers_lq_and_is_seeded() {
    let lossy = LinkParams { loss_good: 0.3, ..LinkParams::ideal(500) };
    let run = |seed| {
        let (mut s, uart) = rig(lossy.clone(), seed);
        s.run_for(2.0).unwrap();
        (uart.take(usize::MAX), get(&s, names::RADIO_LQ))
    };
    let (a, lq) = run(1);
    assert!((55.0..=85.0).contains(&lq), "LQ {lq}");
    let received = rc(&Decoder::default().push(&a)).len();
    assert!((600..=800).contains(&received), "{received} of 1000 packets");
    assert_eq!(a, run(1).0, "same seed, same bytes");
    assert_ne!(a, run(2).0, "different seed, different losses");
}

#[test]
fn a_burst_that_never_ends_loses_everything() {
    let stuck_bad = LinkParams { loss_bad: 1.0, p_good_to_bad: 1.0, p_bad_to_good: 0.0, ..LinkParams::ideal(500) };
    let (mut s, uart) = rig(stuck_bad, 1);
    s.run_for(0.5).unwrap();
    assert!(frames(&uart).is_empty());
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0);
}

#[test]
fn latency_delays_stick_changes_by_whole_packets() {
    let (mut s, uart) = rig(LinkParams { latency_packets: 5, ..LinkParams::ideal(500) }, 1);
    s.run_for(0.1).unwrap();
    uart.take(usize::MAX);
    set(&mut s, names::RC_ROLL, 1.0);
    let mut roll = Vec::new();
    for _ in 0..8 {
        packet(&mut s);
        roll.push(rc(&frames(&uart))[0][0]);
    }
    assert_eq!(roll, vec![992, 992, 992, 992, 992, 1792, 1792, 1792]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ofs-radio --test elrs`
Expected: compile error, no module `elrs`.

- [ ] **Step 3: Implement** `crates/ofs-radio/src/elrs.rs`:

```rust
//! ExpressLRS link, behavioural (fidelity level 1). Every packet the handset samples the sticks; the packet
//! crosses a channel with Gilbert-Elliott burst loss; the receiver writes one CRSF RC frame per packet it
//! receives (and LINK_STATISTICS every N received packets) to the flight controller's UART. When packets
//! stop, the receiver goes silent, as ExpressLRS does by default, and Betaflight's own failsafe takes over.
use std::collections::VecDeque;

use ofs_core::rng::model_rng;
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

use crate::crsf::{self, LinkStatistics, CHANNEL_COUNT};

pub const MODEL_NAME: &str = "radio.elrs";
/// ExpressLRS reports link quality as the share of the last 100 packets received.
pub const LQ_WINDOW: usize = 100;
/// RSSI published while no packet is heard.
pub const NO_SIGNAL_RSSI_DBM: f64 = -130.0;

#[derive(Debug, Clone, PartialEq)]
pub struct LinkParams {
    pub packet_rate_hz: u32,
    /// Packets between the handset sampling the sticks and the receiver outputting them.
    pub latency_packets: u32,
    /// Loss probability per packet in the good and the bad (burst) channel state.
    pub loss_good: f64,
    pub loss_bad: f64,
    /// Per-packet probability of entering and of leaving the bad state.
    pub p_good_to_bad: f64,
    pub p_bad_to_good: f64,
    pub rssi_dbm: f64,
    pub snr_db: f64,
    /// A LINK_STATISTICS frame follows every this many received packets.
    pub link_stats_interval_packets: u32,
    pub rf_mode: u8,
    pub tx_power: u8,
}

impl LinkParams {
    /// A perfect link at `packet_rate_hz`: no loss, one packet of latency.
    pub fn ideal(packet_rate_hz: u32) -> Self {
        Self {
            packet_rate_hz,
            latency_packets: 1,
            loss_good: 0.0,
            loss_bad: 0.0,
            p_good_to_bad: 0.0,
            p_bad_to_good: 1.0,
            rssi_dbm: -50.0,
            snr_db: 10.0,
            link_stats_interval_packets: 50,
            rf_mode: 0,
            tx_power: 0,
        }
    }
}

struct Inputs {
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    fault_loss: Signal<f64>,
}

struct Outputs {
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
}

pub struct ElrsLink {
    params: LinkParams,
    div: u32,
    rng: ChaCha8Rng,
    uart: Wire,
    inputs: Inputs,
    outputs: Outputs,
    bad: bool,
    history: VecDeque<bool>,
    pipeline: VecDeque<[u16; CHANNEL_COUNT]>,
    since_stats: u32,
}

impl ElrsLink {
    /// `uart` receives the receiver's CRSF output (the flight controller's UART RX).
    pub fn new(params: LinkParams, rate_divisor: u32, seed: u64, uart: Wire, bus: &mut Bus) -> Self {
        let inputs = Inputs {
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            fault_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
        };
        let outputs = Outputs {
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
        };
        Self {
            params,
            div: rate_divisor,
            rng: model_rng(seed, MODEL_NAME),
            uart,
            inputs,
            outputs,
            bad: false,
            history: VecDeque::with_capacity(LQ_WINDOW + 1),
            pipeline: VecDeque::new(),
            since_stats: 0,
        }
    }

    /// Handset: sticks to channels in Betaflight's default AETR order, then AUX1..4, the rest centred.
    fn sample(&self, bus: &Bus) -> [u16; CHANNEL_COUNT] {
        let i = &self.inputs;
        let mut ch = [crsf::stick_ticks(0.0); CHANNEL_COUNT];
        ch[0] = crsf::stick_ticks(bus.get(i.roll));
        ch[1] = crsf::stick_ticks(bus.get(i.pitch));
        ch[2] = crsf::throttle_ticks(bus.get(i.throttle));
        ch[3] = crsf::stick_ticks(bus.get(i.yaw));
        for (slot, s) in ch[4..].iter_mut().zip(&i.aux) {
            *slot = crsf::stick_ticks(bus.get(*s));
        }
        ch
    }

    fn link_quality(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        let received = self.history.iter().filter(|r| **r).count();
        100.0 * received as f64 / self.history.len() as f64
    }

    fn statistics(&self, lq: f64) -> LinkStatistics {
        let rssi = (-self.params.rssi_dbm).round().clamp(0.0, 255.0) as u8;
        let snr = self.params.snr_db.round().clamp(-128.0, 127.0) as i8;
        let lq = lq.round().clamp(0.0, 100.0) as u8;
        LinkStatistics {
            uplink_rssi_1: rssi,
            uplink_rssi_2: rssi,
            uplink_lq: lq,
            uplink_snr: snr,
            active_antenna: 0,
            rf_mode: self.params.rf_mode,
            uplink_tx_power: self.params.tx_power,
            downlink_rssi: rssi,
            downlink_lq: lq,
            downlink_snr: snr,
        }
    }
}

impl Model for ElrsLink {
    fn name(&self) -> &str {
        MODEL_NAME
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Handset: the pipeline delays samples by `latency_packets` packets.
        let sample = self.sample(bus);
        if self.pipeline.is_empty() {
            self.pipeline.extend(std::iter::repeat(sample).take(self.params.latency_packets as usize));
        }
        self.pipeline.push_back(sample);
        let channels = self.pipeline.pop_front().expect("the pipeline holds at least this sample");

        // Channel: both numbers are drawn every packet, so the random stream never depends on the inputs.
        let switch: f64 = self.rng.gen();
        let draw: f64 = self.rng.gen();
        self.bad = if self.bad { switch >= self.params.p_bad_to_good } else { switch < self.params.p_good_to_bad };
        let loss = if self.bad { self.params.loss_bad } else { self.params.loss_good };
        let transmitting = bus.get(self.inputs.tx_enabled) > 0.5 && bus.get(self.inputs.fault_loss) < 0.5;
        let received = transmitting && draw >= loss;

        self.history.push_back(received);
        if self.history.len() > LQ_WINDOW {
            self.history.pop_front();
        }
        let lq = self.link_quality();
        let link_up = lq > 0.0;

        // Receiver: one RC frame per received packet, link statistics every N received packets.
        if received {
            self.uart.write(&crsf::rc_channels_frame(&channels));
            self.since_stats += 1;
            if self.since_stats >= self.params.link_stats_interval_packets {
                self.since_stats = 0;
                self.uart.write(&crsf::link_statistics_frame(&self.statistics(lq)));
            }
        }
        bus.set(self.outputs.link_up, if link_up { 1.0 } else { 0.0 });
        bus.set(self.outputs.lq, lq);
        bus.set(self.outputs.rssi, if link_up { self.params.rssi_dbm } else { NO_SIGNAL_RSSI_DBM });
        Ok(())
    }
}
```

Replace `crates/ofs-radio/src/lib.rs` with:

```rust
//! Radio link models: CRSF framing and the ExpressLRS link.
pub mod crsf;
pub mod elrs;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-radio`
Expected: 9 crsf + 7 elrs tests pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-radio
git commit -m "feat(radio): ExpressLRS link model with burst loss, latency and link statistics"
```

---

### Task 4: MSP v1 client

**Files:**
- Create: `crates/ofs-fc/src/msp.rs`, `crates/ofs-fc/tests/msp.rs`
- Modify: `crates/ofs-fc/src/lib.rs`

**Interfaces:**
- Produces, in `ofs_fc::msp`:
  - `MSP_API_VERSION = 1`, `MSP_REBOOT = 68`, `MSP_STATUS = 101`, `MSP_RC = 105`;
  - `encode_request(cmd, &[u8]) -> Vec<u8>`;
  - `MspReply { cmd, payload, error }`;
  - `MspParser::default().push(&[u8]) -> Vec<MspReply>`;
  - `MspError { Io, Timeout { cmd, pumps }, Sim(SimError) }`;
  - `MspClient::connect(SocketAddr, Duration)`, `send`, `poll(cmd) -> Result<Option<MspReply>, MspError>`, `request(cmd, payload, max_pumps, pump: impl FnMut() -> Result<(), SimError>)`;
  - decoders `api_version`, `armed`, `rc_channels_us`.
- SITL services MSP only while simulated time advances, which is why `request` takes a `pump` that steps the simulation.

- [ ] **Step 1: Write the failing tests** `crates/ofs-fc/tests/msp.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use ofs_fc::msp::*;

fn reply(cmd: u8, payload: &[u8], error: bool) -> Vec<u8> {
    let mut f = vec![b'$', b'M', if error { b'!' } else { b'>' }, payload.len() as u8, cmd];
    f.extend_from_slice(payload);
    f.push(payload.iter().fold(payload.len() as u8 ^ cmd, |c, b| c ^ b));
    f
}

#[test]
fn requests_are_framed_with_an_xor_checksum() {
    assert_eq!(encode_request(MSP_API_VERSION, &[]), b"$M<\x00\x01\x01".to_vec());
    assert_eq!(encode_request(200, &[1, 2]), vec![b'$', b'M', b'<', 2, 200, 1, 2, 2 ^ 200 ^ 1 ^ 2]);
}

#[test]
fn parser_skips_noise_and_bad_checksums() {
    let mut bytes = b"noise$M".to_vec();
    let mut bad = reply(MSP_RC, &[1, 2], false);
    *bad.last_mut().unwrap() ^= 0x55;
    bytes.extend(bad);
    bytes.extend(reply(MSP_API_VERSION, &[0, 1, 48], false));
    bytes.extend(reply(MSP_REBOOT, &[], true));
    let mut p = MspParser::default();
    let (a, b) = bytes.split_at(9);
    let mut got = p.push(a);
    got.extend(p.push(b));
    assert_eq!(
        got,
        vec![
            MspReply { cmd: MSP_API_VERSION, payload: vec![0, 1, 48], error: false },
            MspReply { cmd: MSP_REBOOT, payload: vec![], error: true },
        ]
    );
}

#[test]
fn request_pumps_until_the_reply_arrives() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut req = [0u8; 6];
        s.read_exact(&mut req).unwrap();
        assert_eq!(&req, b"$M<\x00\x01\x01");
        s.write_all(&reply(MSP_API_VERSION, &[0, 1, 48], false)).unwrap();
        let mut sink = [0u8; 1];
        let _ = s.read(&mut sink); // hold the connection until the client hangs up
    });
    let mut c = MspClient::connect(addr, Duration::from_secs(2)).unwrap();
    let mut pumps = 0;
    let r = c
        .request(MSP_API_VERSION, &[], 1000, || {
            pumps += 1;
            std::thread::sleep(Duration::from_millis(1));
            Ok(())
        })
        .unwrap();
    assert_eq!(api_version(&r), Some((0, 1, 48)));
    assert!(pumps >= 1);
    drop(c);
    server.join().unwrap();
}

#[test]
fn request_times_out_after_max_pumps() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut sink = [0u8; 64];
        while let Ok(n) = s.read(&mut sink) {
            if n == 0 {
                break;
            }
        }
    });
    let mut c = MspClient::connect(addr, Duration::from_secs(2)).unwrap();
    let err = c.request(MSP_RC, &[], 5, || Ok(())).unwrap_err();
    assert!(matches!(err, MspError::Timeout { cmd: MSP_RC, pumps: 5 }), "{err}");
    drop(c);
    server.join().unwrap();
}

#[test]
fn status_and_rc_decoders() {
    let mut status = vec![0u8; 22];
    status[6] = 1;
    assert_eq!(armed(&MspReply { cmd: MSP_STATUS, payload: status, error: false }), Some(true));
    assert_eq!(armed(&MspReply { cmd: MSP_STATUS, payload: vec![0; 4], error: false }), None);
    let rc = MspReply { cmd: MSP_RC, payload: vec![0x40, 0x06, 0x78, 0x05], error: false };
    assert_eq!(rc_channels_us(&rc), vec![1600, 1400]);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ofs-fc --test msp`
Expected: compile error, no module `msp`.

- [ ] **Step 3: Implement** `crates/ofs-fc/src/msp.rs`:

```rust
//! MSP v1 over TCP: enough to talk to Betaflight SITL's MSP port (UART1, tcp:5761) from tests and tools.
//! SITL services MSP only while simulated time advances, so [`MspClient::request`] pumps the simulation
//! while it waits for the reply.
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use ofs_core::SimError;

pub const MSP_API_VERSION: u8 = 1;
pub const MSP_REBOOT: u8 = 68;
pub const MSP_STATUS: u8 = 101;
pub const MSP_RC: u8 = 105;

fn checksum(len: u8, cmd: u8, payload: &[u8]) -> u8 {
    payload.iter().fold(len ^ cmd, |c, b| c ^ b)
}

/// `$M<` request frame: length, command, payload, XOR checksum of length, command and payload.
pub fn encode_request(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len = u8::try_from(payload.len()).expect("MSP v1 payloads are at most 255 bytes");
    let mut out = b"$M<".to_vec();
    out.push(len);
    out.push(cmd);
    out.extend_from_slice(payload);
    out.push(checksum(len, cmd, payload));
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MspReply {
    pub cmd: u8,
    pub payload: Vec<u8>,
    /// `$M!`: Betaflight did not accept the command.
    pub error: bool,
}

/// Parses `$M>` and `$M!` frames from a byte stream; skips noise and frames with bad checksums.
#[derive(Debug, Default)]
pub struct MspParser {
    buf: Vec<u8>,
}

impl MspParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<MspReply> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            match self.buf.iter().position(|b| *b == b'$') {
                Some(start) => {
                    self.buf.drain(..start);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
            if self.buf.len() < 5 {
                break;
            }
            let error = match &self.buf[1..3] {
                b"M>" => false,
                b"M!" => true,
                _ => {
                    self.buf.remove(0);
                    continue;
                }
            };
            let len = usize::from(self.buf[3]);
            if self.buf.len() < 6 + len {
                break;
            }
            let cmd = self.buf[4];
            let payload = self.buf[5..5 + len].to_vec();
            if checksum(self.buf[3], cmd, &payload) == self.buf[5 + len] {
                out.push(MspReply { cmd, payload, error });
                self.buf.drain(..6 + len);
            } else {
                self.buf.remove(0);
            }
        }
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MspError {
    #[error("MSP I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no MSP reply to command {cmd} after {pumps} simulation steps")]
    Timeout { cmd: u8, pumps: usize },
    #[error(transparent)]
    Sim(#[from] SimError),
}

pub struct MspClient {
    stream: TcpStream,
    parser: MspParser,
    replies: VecDeque<MspReply>,
}

impl MspClient {
    pub fn connect(addr: SocketAddr, timeout: Duration) -> Result<Self, MspError> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Self { stream, parser: MspParser::default(), replies: VecDeque::new() })
    }

    pub fn send(&mut self, cmd: u8, payload: &[u8]) -> Result<(), MspError> {
        let bytes = encode_request(cmd, payload);
        self.stream.set_nonblocking(false)?;
        let written = self.stream.write_all(&bytes);
        self.stream.set_nonblocking(true)?;
        Ok(written?)
    }

    /// Reads whatever has arrived, without blocking; returns the oldest reply to `cmd`, if any. A reply that
    /// arrived just before the connection closed (e.g. MSP_REBOOT's) is still returned.
    pub fn poll(&mut self, cmd: u8) -> Result<Option<MspReply>, MspError> {
        let mut buf = [0u8; 1024];
        let mut closed = false;
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(n) => {
                    let replies = self.parser.push(&buf[..n]);
                    self.replies.extend(replies);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(i) = self.replies.iter().position(|r| r.cmd == cmd) {
            return Ok(self.replies.remove(i));
        }
        if closed {
            return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "MSP connection closed").into());
        }
        Ok(None)
    }

    /// Sends `cmd`, then calls `pump` (which must advance simulated time) until the reply arrives or
    /// `max_pumps` calls have passed.
    pub fn request(
        &mut self,
        cmd: u8,
        payload: &[u8],
        max_pumps: usize,
        mut pump: impl FnMut() -> Result<(), SimError>,
    ) -> Result<MspReply, MspError> {
        self.send(cmd, payload)?;
        for _ in 0..max_pumps {
            pump()?;
            if let Some(reply) = self.poll(cmd)? {
                return Ok(reply);
            }
        }
        Err(MspError::Timeout { cmd, pumps: max_pumps })
    }
}

/// MSP_API_VERSION: (MSP protocol, API major, API minor).
pub fn api_version(reply: &MspReply) -> Option<(u8, u8, u8)> {
    match reply.payload[..] {
        [protocol, major, minor, ..] => Some((protocol, major, minor)),
        _ => None,
    }
}

/// MSP_STATUS: Betaflight's ARM box is bit 0 of the flight-mode flags (u32 at offset 6).
pub fn armed(reply: &MspReply) -> Option<bool> {
    let flags = reply.payload.get(6..10)?;
    Some(u32::from_le_bytes(flags.try_into().ok()?) & 1 == 1)
}

/// MSP_RC: channel values in microseconds, in Betaflight's internal order (roll, pitch, yaw, throttle, aux...).
pub fn rc_channels_us(reply: &MspReply) -> Vec<u16> {
    reply.payload.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}
```

In `crates/ofs-fc/src/lib.rs`, add `pub mod msp;` above `pub mod open_loop;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-fc`
Expected: the 5 msp tests pass, the existing ofs-fc tests still pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-fc
git commit -m "feat(fc): MSP v1 client that pumps the simulation while waiting"
```

---

### Task 5: SITL patch — UART bytes in the state datagram and deterministic boot

**Files:**
- Create: `third_party/betaflight/tools/add_serial_in_datagram.py`, `third_party/betaflight/tools/deterministic_boot.py`
- Modify: `third_party/betaflight/ofs-sitl.patch` (regenerated), `crates/ofs-fc/src/sitl/process.rs`, `crates/ofs-fc/tests/sitl_errors.rs`

**Interfaces:**
- **Produces (SITL side):**
  - SITL accepts `fdm_packet ‖ rc_packet ‖ blocks`, each block `[uart index (0-based)][len lo][len hi][bytes]`, at most 512 bytes of blocks (`EXT_SERIAL_MAX`). The 144- and 184-byte forms keep working.
  - SITL ignores state packets until its scheduler runs, prints `[SITL] ready for the simulator` at that moment, and runs no task before the first packet.
- **Produces (Rust side):**
  - `ofs_fc::sitl::process::{READY_LINE, is_ready_line}`;
  - `SitlProcess::start` returns once SITL printed the ready line (replacing the TCP 5761 probe plus 300 ms sleep);
  - a `bind port … failed` line before it is still a startup error.
- **Why:** see the plan's Decisions 1 and 3. These are C changes to the pinned Betaflight tree, applied by generator scripts so the patch can be regenerated.

- [ ] **Step 1: Write the generator scripts.** `third_party/betaflight/tools/add_serial_in_datagram.py`:

```python
"""Adds "UART bytes in the state datagram" to a Betaflight tree that already has ofs-sitl.patch applied.

The simulator appends blocks of [uart index][length lo][length hi][bytes] after the 184-byte state datagram;
SITL stages them with the packet and hands them to the UART on the tick that applies it, so receiver traffic
(CRSF) is deterministic in lockstep. See docs/research/sitl-interface.md §8.

Usage (Linux or WSL), from the repository root:
    bash scripts/build-sitl.sh                                   # tree at the pinned commit + current patch
    python3 third_party/betaflight/tools/add_serial_in_datagram.py ~/ofs/betaflight
    git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
    bash scripts/build-sitl.sh                                   # rebuild from the regenerated patch
"""
import sys

ROOT = sys.argv[1]


def edit(rel, replacements):
    path = f"{ROOT}/{rel}"
    with open(path, newline="") as f:
        text = f.read()
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: anchor not found exactly once:\n{old}"
        text = text.replace(old, new, 1)
    with open(path, "w", newline="") as f:
        f.write(text)


edit("src/main/drivers/serial_tcp.c", [(
    r"""            s->port.rxCallback(ch, s->port.rxCallbackData);
        }
    }
}
""",
    r"""            s->port.rxCallback(ch, s->port.rxCallbackData);
        }
    }
}

// Lockstep builds: bytes for UART `id` (0-based) that the simulator sent inside the state datagram. They join
// the port's RX buffer like bytes from a TCP client and reach the driver at this tick's tcpSerialDispatchRx().
void tcpSerialInject(unsigned id, const uint8_t *data, int size)
{
    if (id >= ARRAYLEN(tcpSerialPorts) || !tcpPortInitialized[id] || size <= 0) {
        return;
    }
    tcpDataIn(&tcpSerialPorts[id], (uint8_t *)data, size);
}
""")])

edit("src/main/drivers/serial_tcp.h", [(
    "void tcpSerialDispatchRx(void);  // lockstep: deliver buffered RX bytes on the main thread\n",
    "void tcpSerialDispatchRx(void);  // lockstep: deliver buffered RX bytes on the main thread\n"
    "void tcpSerialInject(unsigned id, const uint8_t *data, int size);  // lockstep: UART bytes from the state datagram\n",
)])

edit("src/platform/SIMULATOR/sitl.c", [
    (
        "static uint16_t extFdmRcChannels[SIMULATOR_MAX_RC_CHANNELS];\n",
        r"""static uint16_t extFdmRcChannels[SIMULATOR_MAX_RC_CHANNELS];

// UART bytes carried in the state datagram after the rc_packet: blocks of [uart index][len lo][len hi][bytes].
// Staged with the packet and handed to the UARTs on the tick that applies it, so receiver traffic (CRSF) is
// deterministic in lockstep. FDM thread: extFdmSerial*; under extPacketMutex: extSerialPending*.
#define EXT_SERIAL_MAX 512
static uint8_t extFdmSerial[EXT_SERIAL_MAX];
static int extFdmSerialLen = 0;
static uint8_t extSerialPending[EXT_SERIAL_MAX];
static int extSerialPendingLen = 0;

static void extInjectSerial(const uint8_t *p, int len)
{
    int i = 0;
    while (i + 3 <= len) {
        const unsigned uart = p[i];
        const int n = p[i + 1] | (p[i + 2] << 8);
        i += 3;
        if (n > len - i) {
            break;
        }
        tcpSerialInject(uart, &p[i], n);
        i += n;
    }
}
""",
    ),
    (
        """    uint16_t rc[SIMULATOR_MAX_RC_CHANNELS];
    bool rcNew = false;
""",
        """    uint16_t rc[SIMULATOR_MAX_RC_CHANNELS];
    bool rcNew = false;
    uint8_t serial[EXT_SERIAL_MAX];
    int serialLen = 0;
""",
    ),
    (
        """            extRcPending = false;
        }
    }
    pthread_mutex_unlock(&extPacketMutex);
""",
        """            extRcPending = false;
        }
        serialLen = extSerialPendingLen;
        if (serialLen > 0) {
            memcpy(serial, extSerialPending, serialLen);
            extSerialPendingLen = 0;
        }
    }
    pthread_mutex_unlock(&extPacketMutex);
""",
    ),
    (
        """    tcpSerialDispatchRx();
    extReplyPending = true;
""",
        """    extInjectSerial(serial, serialLen);
    tcpSerialDispatchRx();
    extReplyPending = true;
""",
    ),
    (
        """        extFdmRcValid = false;
    }
    extGyroTicks++;
""",
        """        extFdmRcValid = false;
    }
    if (extFdmSerialLen > 0) {
        const int room = EXT_SERIAL_MAX - extSerialPendingLen;
        const int n = extFdmSerialLen < room ? extFdmSerialLen : room;
        memcpy(extSerialPending + extSerialPendingLen, extFdmSerial, n);
        extSerialPendingLen += n;
        extFdmSerialLen = 0;
    }
    extGyroTicks++;
""",
    ),
    (
        """        static struct { fdm_packet fdm; rc_packet rc; } __attribute__((packed)) fdmRcPkt;
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n == sizeof(fdmRcPkt)) {
            memcpy(extFdmRcChannels, fdmRcPkt.rc.channels, sizeof(extFdmRcChannels));
            extFdmRcValid = true;
        }
        if (n == sizeof(fdm_packet) || n == sizeof(fdmRcPkt)) {
""",
        """        // UART blocks may follow the rc_packet (see EXT_SERIAL_MAX).
        static struct { fdm_packet fdm; rc_packet rc; uint8_t serial[EXT_SERIAL_MAX]; } __attribute__((packed)) fdmRcPkt;
        const int fdmRcSize = (int)(sizeof(fdm_packet) + sizeof(rc_packet));
        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n >= fdmRcSize) {
            memcpy(extFdmRcChannels, fdmRcPkt.rc.channels, sizeof(extFdmRcChannels));
            extFdmRcValid = true;
            extFdmSerialLen = n - fdmRcSize;
            memcpy(extFdmSerial, fdmRcPkt.serial, extFdmSerialLen);
        }
        if (n == (int)sizeof(fdm_packet) || n >= fdmRcSize) {
""",
    ),
])
print("serial-in-datagram applied to", ROOT)
```

`third_party/betaflight/tools/deterministic_boot.py`:

```python
"""Makes lockstep SITL boot the same way every time.

Two races made runs differ (found in M2 with the CRSF receiver; docs/research/sitl-interface.md §8):
- SITL's UDP thread starts early in systemInit(), so a state packet could land while init still ran, and init
  read the fake sensors either before or after the packet's values arrived;
- before the first packet the scheduler ran tasks on wall time (gyro calibration, attitude estimate).

Now state packets are ignored until init has finished and the scheduler runs (which SITL announces with
"[SITL] ready for the simulator"), and the scheduler runs no task until the first packet.

Usage (Linux or WSL) on a tree with ofs-sitl.patch applied:
    python3 third_party/betaflight/tools/deterministic_boot.py ~/ofs/betaflight
"""
import sys

ROOT = sys.argv[1]


def edit(rel, replacements):
    path = f"{ROOT}/{rel}"
    with open(path, newline="") as f:
        text = f.read()
    for old, new in replacements:
        assert text.count(old) == 1, f"{rel}: anchor not found exactly once:\n{old}"
        text = text.replace(old, new, 1)
    with open(path, "w", newline="") as f:
        f.write(text)


edit("src/platform/SIMULATOR/target/SITL/target.h", [(
    "bool simulatorTakeGyroTick(void);\n",
    "bool simulatorTakeGyroTick(void);\nbool simulatorTimeStarted(void);\n",
)])

edit("src/platform/SIMULATOR/sitl.c", [
    (
        "// Called by the scheduler: true once per FDM packet.",
        r"""// Set when the scheduler first runs (init has finished). State packets before that are ignored, so init
// never sees a packet's sensor values or time: every boot is the same.
static bool extSchedulerRunning = false;

// False until the first FDM packet is staged: until then the scheduler runs no task, so nothing in boot
// (gyro calibration, attitude estimate) depends on wall time.
bool simulatorTimeStarted(void)
{
    if (!__atomic_load_n(&extSchedulerRunning, __ATOMIC_ACQUIRE)) {
        __atomic_store_n(&extSchedulerRunning, true, __ATOMIC_RELEASE);
        printf("[SITL] ready for the simulator\n");
    }
    pthread_mutex_lock(&extPacketMutex);
    const bool started = extGyroTicks != 0;
    pthread_mutex_unlock(&extPacketMutex);
    return started;
}

// Called by the scheduler: true once per FDM packet.""",
    ),
    (
        "        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);\n",
        """        n = udpRecv(&stateLink, &fdmRcPkt, sizeof(fdmRcPkt), 100);
        if (n > 0 && !__atomic_load_n(&extSchedulerRunning, __ATOMIC_ACQUIRE)) {
            continue;  // still initialising: the simulator resends (see extSchedulerRunning)
        }
""",
    ),
])

edit("src/main/scheduler/scheduler.c", [(
    "FAST_CODE void scheduler(void)\n{\n",
    """FAST_CODE void scheduler(void)
{
#if defined(ENABLE_SIMULATOR_EXTERNAL_TIME) && ENABLE_SIMULATOR_EXTERNAL_TIME
    if (!simulatorTimeStarted()) {
        delayMicroseconds(1000);  // no simulated time yet: run no task on wall time
        return;
    }
#endif
""",
)])
print("deterministic boot applied:", ROOT)
```

- [ ] **Step 2: Regenerate and rebuild the patch** (Linux or WSL; from Windows wrap it in `wsl.exe -d Ubuntu -e bash -lc '…'`):

```bash
cd /mnt/c/dev/open-fpv-sim     # or the repository root on Linux
bash scripts/build-sitl.sh     # clean pinned tree + the current patch, built
python3 third_party/betaflight/tools/add_serial_in_datagram.py ~/ofs/betaflight
python3 third_party/betaflight/tools/deterministic_boot.py ~/ofs/betaflight
git -C ~/ofs/betaflight diff --stat | tail -1
git -C ~/ofs/betaflight diff > third_party/betaflight/ofs-sitl.patch
bash scripts/build-sitl.sh     # proves the regenerated patch applies to a clean tree and builds
```

Expected:
- the diff stat reads `8 files changed, 428 insertions(+), 14 deletions(-)`;
- the second build ends with `obj/main/betaflight_SITL.elf` listed;
- `grep -c "warning:"` on the build output is 0 for `sitl.c`, `serial_tcp.c` and `scheduler.c`;
- the patch file has LF line endings (`.gitattributes` enforces it).

- [ ] **Step 3: Write the failing test.** Append to `crates/ofs-fc/tests/sitl_errors.rs`:

```rust

#[test]
fn ready_lines_are_recognised() {
    assert!(is_ready_line("[SITL] ready for the simulator"));
    assert!(!is_ready_line("bind port 5761 for UART1"));
}
```

and change its import to `use ofs_fc::sitl::process::{is_bind_failure, is_ready_line, LaunchConfig};`.

Run: `cargo test -p ofs-fc --test sitl_errors`
Expected: compile error, no `is_ready_line`.

- [ ] **Step 4: Implement the ready-line wait** in `crates/ofs-fc/src/sitl/process.rs`.
  1. Delete the imports `use std::net::{SocketAddr, TcpStream};` and `use super::codec::MSP_TCP_PORT;`.
  2. After `is_bind_failure`, add:

```rust

/// The patched SITL prints this when init has finished and its scheduler runs; only then does it accept
/// state packets (third_party/betaflight/ofs-sitl.patch: deterministic boot).
pub const READY_LINE: &str = "[SITL] ready for the simulator";

pub fn is_ready_line(line: &str) -> bool {
    line.contains(READY_LINE)
}
```

  3. Add `ready_seen: bool,` to `LogSink` after `bind_failed: bool,`, and in `LogSink::push`, after the `is_bind_failure` check:

```rust
        if is_ready_line(&line) {
            self.ready_seen = true;
        }
```

  4. Replace the whole `fn wait_until_ready` with:

```rust
    /// Waits for SITL to announce it is ready ([`READY_LINE`]). A stale instance holding the ports makes the new one
    /// print `bind port ... failed` during init, before that line: a startup error.
    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                return Err(FcError::Startup(format!("exited with {status} during startup"), self.log_tail()));
            }
            let (ready, bind_failed) = self.log.lock().map(|l| (l.ready_seen, l.bind_failed)).unwrap_or((false, false));
            if bind_failed {
                let what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)";
                return Err(FcError::Startup(what.into(), self.log_tail()));
            }
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let what = format!(
                    "did not print \"{READY_LINE}\" within {} ms (is it the lockstep build from scripts/build-sitl.sh?)",
                    timeout.as_millis()
                );
                return Err(FcError::Startup(what, self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ofs-fc` (no SITL needed).
Expected: all pass, including `ready_lines_are_recognised`; no warnings.

- [ ] **Step 6: Verify against the rebuilt SITL** (from Windows, with `OFS_SITL_LAUNCH` set to the WSL form). Run each command 3 times:

```bash
cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1
cargo build -p ofs-sim && python -m pytest python/tests/test_sitl_hover.py -v
```

Expected:
- every run passes;
- the hover's determinism test passes;
- no "resent" warning appears in the ofs-sim output;
- no SITL is left afterwards (`wsl.exe -d Ubuntu -e pgrep -x betaflight_SITL` prints nothing).

On Linux, run the same `sitl_live` command natively with `OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf`.

- [ ] **Step 7: Commit**

```bash
git add third_party/betaflight crates/ofs-fc
git commit -m "feat(sitl): UART bytes in the state datagram and a deterministic boot handshake"
```

---

### Task 6: The bridge carries UART bytes (CRSF reaches Betaflight)

**Files:**
- Modify: `crates/ofs-fc/src/sitl/codec.rs`, `crates/ofs-fc/src/sitl/bridge.rs`, `crates/ofs-fc/Cargo.toml`, `crates/ofs-fc/tests/sitl_codec.rs`, `crates/ofs-fc/tests/sitl_errors.rs`, `crates/ofs-sim/src/vehicle.rs`
- Replace: `crates/ofs-fc/tests/sitl_live.rs`
- Create: `crates/ofs-fc/tests/data/crsf.betaflight.diff`

**Interfaces:**
- Consumes:
  - the SITL datagram extension (Task 5);
  - `ofs_core::Wire` (Task 1);
  - in tests: `ofs_radio::crsf` (Task 2) and `ofs_fc::msp` (Task 4).
- Produces:
  - `codec::{SERIAL_SECTION_MAX = 512, SERIAL_BLOCK_HEADER = 3, state_datagram_with_serial(&FdmPacket, &RcPacket, &[(u8, Vec<u8>)]) -> Vec<u8>}`;
  - `bridge::SerialLink { uart_index: u8 /* 0-based */, rx: Wire }`;
  - `BridgeConfig.serial: Vec<SerialLink>`. The bridge drains each link's wire into every datagram, staying within 512 bytes; bytes beyond that wait for the next exchange.

- [ ] **Step 1: Write the failing codec tests.** In `crates/ofs-fc/tests/sitl_codec.rs`, change the codec import to:

```rust
use ofs_fc::sitl::codec::{
    state_datagram, state_datagram_with_serial, FdmPacket, RcPacket, ServoPacket, SERIAL_SECTION_MAX, STATE_DATAGRAM_SIZE,
};
```

and append:

```rust

fn zero_fdm() -> FdmPacket {
    FdmPacket {
        timestamp_s: 0.25,
        gyro_rpy_radps: [0.0; 3],
        accel_xyz_mps2: [0.0; 3],
        quat_wxyz: [1.0, 0.0, 0.0, 0.0],
        velocity_xyz_mps: [0.0; 3],
        position_xyz: [0.0; 3],
        pressure_pa: 101_325.0,
    }
}

#[test]
fn serial_blocks_follow_the_state_datagram() {
    let fdm = zero_fdm();
    let rc = RcPacket { timestamp_s: 0.25, channels: [0; 16] };
    assert_eq!(state_datagram_with_serial(&fdm, &rc, &[]), state_datagram(&fdm, &rc).to_vec());
    let d = state_datagram_with_serial(&fdm, &rc, &[(1, vec![0xC8, 0x18, 0x16]), (4, vec![0xAA])]);
    assert_eq!(&d[..STATE_DATAGRAM_SIZE], &state_datagram(&fdm, &rc)[..]);
    assert_eq!(&d[STATE_DATAGRAM_SIZE..], &[1, 3, 0, 0xC8, 0x18, 0x16, 4, 1, 0, 0xAA]);
}

#[test]
#[should_panic(expected = "EXT_SERIAL_MAX")]
fn oversized_serial_sections_are_refused() {
    let rc = RcPacket { timestamp_s: 0.0, channels: [0; 16] };
    state_datagram_with_serial(&zero_fdm(), &rc, &[(1, vec![0; SERIAL_SECTION_MAX])]);
}
```

Run: `cargo test -p ofs-fc --test sitl_codec`
Expected: compile error, no `state_datagram_with_serial`.

- [ ] **Step 2: Implement the codec.** In `crates/ofs-fc/src/sitl/codec.rs`, insert before `#[derive(Debug, Clone, Copy, PartialEq)] pub struct ServoPacket {`:

```rust
/// Bytes for SITL's UARTs may follow the 184-byte state datagram, as blocks of
/// `[uart index (0-based)][length, u16 little-endian][bytes]`. The patched SITL hands them to the UART on the
/// tick that applies the packet, so receiver traffic is deterministic. At most this many bytes of blocks
/// (headers included) per datagram: `EXT_SERIAL_MAX` in third_party/betaflight/ofs-sitl.patch.
pub const SERIAL_SECTION_MAX: usize = 512;
pub const SERIAL_BLOCK_HEADER: usize = 3;

/// The state datagram followed by serial blocks `(uart index, bytes)`.
pub fn state_datagram_with_serial(fdm: &FdmPacket, rc: &RcPacket, blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut out = state_datagram(fdm, rc).to_vec();
    for (uart, bytes) in blocks {
        let len = u16::try_from(bytes.len()).expect("serial block too long");
        out.push(*uart);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(bytes);
    }
    assert!(out.len() - STATE_DATAGRAM_SIZE <= SERIAL_SECTION_MAX, "serial section exceeds SITL's EXT_SERIAL_MAX");
    out
}

```

Run: `cargo test -p ofs-fc --test sitl_codec`
Expected: all pass.

- [ ] **Step 3: Add serial links to the bridge.** In `crates/ofs-fc/src/sitl/bridge.rs`:
  1. Change `use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};` to `use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};`.
  2. Change the codec import to:

```rust
use super::codec::{
    state_datagram_with_serial, RcPacket, ServoPacket, PORT_PWM, PORT_STATE, SERIAL_BLOCK_HEADER, SERIAL_SECTION_MAX,
};
```

  3. In `pub struct BridgeConfig`, after `pub motor_count: usize,` add:

```rust
    /// Bytes for SITL's UARTs, carried in the state datagram (e.g. the receiver's CRSF into UART2).
    pub serial: Vec<SerialLink>,
```

  4. After the `BridgeConfig` struct, add:

```rust

/// Bytes the simulator feeds into one of SITL's UARTs.
#[derive(Debug, Clone)]
pub struct SerialLink {
    /// 0-based: UART2 = 1.
    pub uart_index: u8,
    pub rx: Wire,
}
```

  5. In `impl SitlBridge`, after `fn firmware_error`, add:

```rust

    /// Takes pending UART bytes for this datagram, within SITL's serial section limit.
    fn serial_blocks(&self) -> Vec<(u8, Vec<u8>)> {
        let mut blocks = Vec::new();
        let mut room = SERIAL_SECTION_MAX;
        for link in &self.cfg.serial {
            if room <= SERIAL_BLOCK_HEADER {
                break;
            }
            let bytes = link.rx.take(room - SERIAL_BLOCK_HEADER);
            if !bytes.is_empty() {
                room -= SERIAL_BLOCK_HEADER + bytes.len();
                blocks.push((link.uart_index, bytes));
            }
        }
        blocks
    }
```

  6. In `step`, replace `let datagram = state_datagram(&fdm_packet(&frame, &self.cfg.home), &rc);` with:

```rust
        let datagram = state_datagram_with_serial(&fdm_packet(&frame, &self.cfg.home), &rc, &self.serial_blocks());
```

Then add `serial: vec![],` after `motor_count: n,` in the `BridgeConfig` literal in `crates/ofs-sim/src/vehicle.rs` (the vehicle wires the radio in Task 8). Add it after `motor_count: 4,` in `config()` in `crates/ofs-fc/tests/sitl_errors.rs` too.

- [ ] **Step 4: Live test that CRSF in the datagram drives Betaflight.** Add `ofs-radio.workspace = true` to `[dev-dependencies]` in `crates/ofs-fc/Cargo.toml` (before `tempfile`). Create `crates/ofs-fc/tests/data/crsf.betaflight.diff`:

```text
# CRSF receiver on UART2 (serial index 1), fed by serial blocks in the state datagram.
feature -GPS
feature RX_SERIAL
serial 1 64 115200 57600 0 115200
set serialrx_provider = CRSF
aux 0 0 0 1700 2100 0 0
aux 1 1 1 1700 2100 0 0
set motor_pwm_protocol = PWM
set small_angle = 180
```

Replace `crates/ofs-fc/tests/sitl_live.rs` with:

```rust
//! Needs a built Betaflight SITL. Run with:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Scheduler, SimError, StepCtx, Wire};
use ofs_fc::msp::{rc_channels_us, MspClient, MSP_RC};
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SitlBridge};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_radio::crsf;

const QUAD_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.betaflight.diff");
const CRSF_DIFF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/crsf.betaflight.diff");

fn argv(var: &str) -> Vec<String> {
    std::env::var(var).unwrap_or_default().split_whitespace().map(String::from).collect()
}

/// A level, still quad on the bus and a bridge to a fresh SITL (working directory under `dir`).
/// `source`, if any, runs before the bridge in every tick.
fn still_quad(dir: &Path, diff: &str, serial: Vec<SerialLink>, source: Option<Box<dyn Model>>) -> Scheduler {
    let launch = argv("OFS_SITL_LAUNCH");
    assert!(!launch.is_empty(), "set OFS_SITL_LAUNCH");
    let mut cleanup = argv("OFS_SITL_CLEANUP");
    if cleanup.is_empty() {
        cleanup = net::default_cleanup(&launch);
    }
    let sitl_net = net::resolve(&launch, None, None).unwrap();
    let mut bus = Bus::new();
    let accel = bus.signal::<DVec3>(names::IMU_ACCEL);
    bus.set(accel, DVec3::new(0.0, 0.0, -9.80665));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let pressure = bus.signal::<f64>(names::BARO_PRESSURE);
    bus.set(pressure, 101_325.0);
    let bridge = SitlBridge::start(
        BridgeConfig {
            launch: LaunchConfig {
                launch,
                cleanup,
                workdir: dir.join("fc"),
                diff_file: diff.into(),
                startup_timeout: Duration::from_secs(15),
            },
            net: sitl_net,
            rate_divisor: 8,
            first_reply_timeout: Duration::from_secs(5),
            reply_timeout: Duration::from_millis(500),
            home: Home { lat_deg: 50.85, lon_deg: 4.35, alt_m: 30.0 },
            motor_count: 4,
            serial,
        },
        &mut bus,
    )
    .unwrap();
    let mut s = Scheduler::new(8000, bus);
    if let Some(m) = source {
        s.add(m);
    }
    s.add(Box::new(bridge));
    s
}

#[test]
#[ignore]
fn still_quad_exchanges_packets_and_stays_disarmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = still_quad(dir.path(), QUAD_DIFF, vec![], None);
    s.run_for(2.0).unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 0.0);
    }
}

/// Writes one CRSF RC frame per step, like a receiver at 500 Hz.
struct CrsfSource {
    uart: Wire,
    channels: [u16; crsf::CHANNEL_COUNT],
}

impl Model for CrsfSource {
    fn name(&self) -> &str {
        "test.crsf_source"
    }

    fn rate_divisor(&self) -> u32 {
        16
    }

    fn step(&mut self, _ctx: &StepCtx, _bus: &mut Bus) -> Result<(), SimError> {
        self.uart.write(&crsf::rc_channels_frame(&self.channels));
        Ok(())
    }
}

#[test]
#[ignore]
fn crsf_bytes_in_the_state_datagram_reach_betaflight() {
    let dir = tempfile::tempdir().unwrap();
    let uart = Wire::new(4096);
    let mut channels = [crsf::stick_ticks(0.0); crsf::CHANNEL_COUNT];
    channels[0] = crsf::stick_ticks(0.2); // roll -> 1600 us
    channels[1] = crsf::stick_ticks(-0.2); // pitch -> 1400 us
    channels[2] = crsf::throttle_ticks(0.0); // throttle -> 1000 us
    channels[3] = crsf::stick_ticks(0.0); // yaw -> 1500 us
    let link = SerialLink { uart_index: 1, rx: uart.clone() }; // UART2
    let mut s = still_quad(dir.path(), CRSF_DIFF, vec![link], Some(Box::new(CrsfSource { uart, channels })));
    s.run_for(2.0).unwrap();
    let mut msp = MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap();
    let reply = msp.request(MSP_RC, &[], 500, || s.run_for(0.01)).unwrap();
    // MSP_RC lists roll, pitch, yaw, throttle (Betaflight's internal order).
    assert_eq!(&rc_channels_us(&reply)[..4], &[1600, 1400, 1500, 1000]);
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace`
Expected: all pass, no warnings.

Then run the live test with `OFS_SITL_LAUNCH` set, 3 times:

```bash
cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1
```

Expected: 2 passed every time. `crsf_bytes_in_the_state_datagram_reach_betaflight` must read exactly `[1600, 1400, 1500, 1000]`.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-fc crates/ofs-sim/src/vehicle.rs
git commit -m "feat(fc): carry UART bytes to Betaflight inside the lockstep datagram"
```

---

### Task 7: Radio section in the quad file, per-file firmware directories, stale-diff guard

**Files:**
- Modify:
  - `crates/ofs-config/src/lib.rs`, `crates/ofs-config/tests/config.rs`;
  - `quads/opendrone-5f-freestyle.toml`;
  - `crates/ofs-fc/src/sitl/process.rs`, `crates/ofs-fc/tests/sitl_errors.rs`;
  - `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/tests/open_loop_vehicle.rs`;
  - `python/tests/test_sitl_hover.py`

**Interfaces:**
- **Produces:**
  - `ofs_config::{SCHEMA_VERSION = 2, RadioKind::Elrs, RadioSection { kind, packet_rate_hz, uart, latency_packets, loss_good, loss_bad, p_good_to_bad, p_bad_to_good, rssi_dbm, snr_db, link_stats_interval_packets, rf_mode, tx_power }}` and `QuadConfig.radio`;
  - `ofs_sim::vehicle::firmware_dir(data_dir: &Path, quad_path: &Path) -> PathBuf`.
- **Behaviour:** `SitlProcess::start` refuses with `FcError::Config` when `eeprom.bin` exists and the quad's diff differs from the applied copy (`<workdir>/betaflight.diff`).
- **Validation, reported together:**
  - `radio.packet_rate_hz` divides `sim.base_hz`;
  - `radio.uart` is in 2..=8 (UART1 is the MSP port);
  - probabilities are in [0, 1];
  - `rssi_dbm` is ≤ 0 and finite;
  - `snr_db` is finite;
  - `link_stats_interval_packets` is > 0.

- [ ] **Step 1: Write the failing config tests.** In `crates/ofs-config/tests/config.rs`:
  1. Change the import to `use ofs_config::{load, ConfigError, FcKind, RadioKind};`.
  2. In `reference_quad_loads_and_validates`, replace `assert_eq!(cfg.schema_version, 1);` with:

```rust
    assert_eq!(cfg.schema_version, 2);
    assert_eq!(cfg.radio.kind, RadioKind::Elrs);
    assert_eq!((cfg.radio.packet_rate_hz, cfg.radio.uart), (500, 2));
```

  3. In `unsupported_schema_version_is_explicit`, replace the body's text replacement and assertion with:

```rust
    let text = quad_text().replace("schema_version = 2", "schema_version = 1");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    assert!(err.to_string().contains("schema_version 1"), "{err}");
```

  4. Append:

```rust

#[test]
fn radio_problems_are_reported_together() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text()
        .replace("packet_rate_hz = 500", "packet_rate_hz = 333")
        .replace("uart = 2", "uart = 1")
        .replace("rssi_dbm = -50.0", "rssi_dbm = -50.0\nloss_good = 1.5");
    let err = load(&write_quad(dir.path(), &text)).unwrap_err();
    let ConfigError::Invalid { problems, .. } = &err else { panic!("expected Invalid, got {err}") };
    let fields: Vec<&str> = problems.iter().map(|p| p.field.as_str()).collect();
    for field in ["radio.packet_rate_hz", "radio.uart", "radio.loss_good"] {
        assert!(fields.contains(&field), "{field} missing from {fields:?}");
    }
}

#[test]
fn a_quad_without_a_radio_is_a_parse_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let text = quad_text();
    let without = &text[..text.find("[radio]").unwrap()];
    let err = load(&write_quad(dir.path(), without)).unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("radio"), "{err}");
}
```

Run: `cargo test -p ofs-config`
Expected: compile errors (`RadioKind`, `cfg.radio`).

- [ ] **Step 2: Implement the radio section.** In `crates/ofs-config/src/lib.rs`:
  1. Replace `pub const SCHEMA_VERSION: u32 = 1;` with:

```rust
/// 2: adds the required `[radio]` section (M2).
pub const SCHEMA_VERSION: u32 = 2;
```

  2. In `pub struct QuadConfig`, after `pub fc: FcSection,` add `pub radio: RadioSection,`.
  3. Insert before `#[derive(Debug, Clone, PartialEq)] pub struct Problem {`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioKind {
    /// ExpressLRS, behavioural model (fidelity level 1).
    Elrs,
}

/// The pilot's radio link. The receiver's CRSF output is wired to a Betaflight UART.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioSection {
    pub kind: RadioKind,
    pub packet_rate_hz: u32,
    /// Betaflight UART number the receiver is wired to (1-based, as in the Configurator's Ports tab).
    pub uart: u8,
    #[serde(default = "default_latency_packets")]
    pub latency_packets: u32,
    /// Per-packet loss probability in the good and the bad (burst) channel state.
    #[serde(default)]
    pub loss_good: f64,
    #[serde(default)]
    pub loss_bad: f64,
    /// Per-packet probability of entering and of leaving the bad state (Gilbert-Elliott burst loss).
    #[serde(default)]
    pub p_good_to_bad: f64,
    #[serde(default = "default_p_bad_to_good")]
    pub p_bad_to_good: f64,
    #[serde(default = "default_rssi_dbm")]
    pub rssi_dbm: f64,
    #[serde(default = "default_snr_db")]
    pub snr_db: f64,
    #[serde(default = "default_link_stats_interval_packets")]
    pub link_stats_interval_packets: u32,
    /// Reported as-is in CRSF link statistics.
    #[serde(default)]
    pub rf_mode: u8,
    #[serde(default)]
    pub tx_power: u8,
}

fn default_latency_packets() -> u32 {
    1
}

fn default_p_bad_to_good() -> f64 {
    1.0
}

fn default_rssi_dbm() -> f64 {
    -50.0
}

fn default_snr_db() -> f64 {
    10.0
}

fn default_link_stats_interval_packets() -> u32 {
    50
}

```

  4. In `validate`, insert before `if self.fc.kind == FcKind::Sitl {`:

```rust
        let r = &self.radio;
        c.divides(self.sim.base_hz, r.packet_rate_hz, "radio.packet_rate_hz");
        c.check(
            (2..=8).contains(&r.uart),
            "radio.uart",
            format!("must be 2..=8; UART1 is Betaflight's MSP port, tcp:5761 (got {})", r.uart),
        );
        for (p, field) in [
            (r.loss_good, "radio.loss_good"),
            (r.loss_bad, "radio.loss_bad"),
            (r.p_good_to_bad, "radio.p_good_to_bad"),
            (r.p_bad_to_good, "radio.p_bad_to_good"),
        ] {
            c.check((0.0..=1.0).contains(&p), field, format!("must be a probability in [0, 1] (got {p})"));
        }
        c.check(r.rssi_dbm.is_finite() && r.rssi_dbm <= 0.0, "radio.rssi_dbm", format!("must be <= 0 dBm (got {})", r.rssi_dbm));
        c.check(r.snr_db.is_finite(), "radio.snr_db", "must be finite");
        c.check(r.link_stats_interval_packets > 0, "radio.link_stats_interval_packets", "must be > 0");
```

In `quads/opendrone-5f-freestyle.toml`, change `schema_version = 1` to `schema_version = 2`, and append:

```toml

[radio]
# ExpressLRS, behavioural (level 1). The receiver's CRSF output is wired to Betaflight's UART2; the
# betaflight.diff selects the CRSF receiver there.
kind = "elrs"
packet_rate_hz = 500
uart = 2
latency_packets = 1
rssi_dbm = -50.0
snr_db = 10.0
```

Run: `cargo test -p ofs-config`
Expected: all pass.

- [ ] **Step 3: Write the failing stale-diff test.** Append to `crates/ofs-fc/tests/sitl_errors.rs`:

```rust

#[test]
fn a_changed_quad_diff_refuses_a_stale_eeprom() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), &["ofs-definitely-missing-binary"]);
    // First boot happened with another diff: eeprom.bin and the applied copy exist.
    std::fs::create_dir_all(dir.path().join("fc")).unwrap();
    std::fs::write(dir.path().join("fc/eeprom.bin"), [0u8; 16]).unwrap();
    std::fs::write(dir.path().join("fc/betaflight.diff"), "feature GPS\n").unwrap();
    let err = SitlBridge::start(cfg, &mut Bus::new()).err().unwrap();
    assert!(matches!(err, FcError::Config(_)), "{err}");
    let shown = err.to_string();
    assert!(shown.contains("changed since") && shown.contains("eeprom.bin"), "{shown}");
}
```

Run: `cargo test -p ofs-fc --test sitl_errors`
Expected: FAIL. The error is `Launch` (the missing binary), not `Config`.

- [ ] **Step 4: Implement the guard.** In `crates/ofs-fc/src/sitl/process.rs`, in `SitlProcess::start`, replace:

```rust
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
```

with:

```rust
        if workdir.join("eeprom.bin").exists() {
            check_diff_unchanged(cfg, &workdir)?;
        }
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
```

and add before `/// First boot: \`<launch> --config betaflight.diff\` …`:

```rust
/// The quad's diff is applied on first boot only, so an EEPROM made from an older diff no longer matches the
/// quad file. Refuse to start rather than fly a stale configuration.
fn check_diff_unchanged(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    let Ok(applied) = std::fs::read(workdir.join("betaflight.diff")) else {
        return Ok(()); // first booted before copies of the applied diff were kept
    };
    if applied != std::fs::read(&cfg.diff_file)? {
        return Err(FcError::Config(format!(
            "{} changed since this quad's first boot; delete {} to apply it again (this also discards settings \
             changed in Betaflight Configurator)",
            cfg.diff_file.display(),
            workdir.join("eeprom.bin").display()
        )));
    }
    Ok(())
}

```

Run: `cargo test -p ofs-fc --test sitl_errors`
Expected: all pass.

- [ ] **Step 5: Write the failing firmware-directory test.** In `crates/ofs-sim/tests/open_loop_vehicle.rs`, change the import to `use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Sticks, Vehicle};` and append:

```rust

#[test]
fn firmware_dirs_differ_for_same_named_quads_in_different_folders() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    for d in [&a, &b] {
        std::fs::write(d.path().join("quad.toml"), "").unwrap();
    }
    let data = Path::new("data");
    let da = firmware_dir(data, &a.path().join("quad.toml"));
    let db = firmware_dir(data, &b.path().join("quad.toml"));
    assert_ne!(da, db);
    assert!(da.file_name().unwrap().to_string_lossy().starts_with("quad-"), "{da:?}");
    assert_eq!(da, firmware_dir(data, &a.path().join("quad.toml")), "stable");
}
```

Run: `cargo test -p ofs-sim --test open_loop_vehicle`
Expected: compile error, no `firmware_dir`.

- [ ] **Step 6: Implement it.** In `crates/ofs-sim/src/vehicle.rs`:
  1. Change `use std::path::PathBuf;` to `use std::path::{Path, PathBuf};`.
  2. Add `use ofs_core::rng::fnv1a64;` above `use ofs_core::{names, …};`.
  3. After `pub struct Vehicle { … }`, add:

```rust

/// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
/// the same file name in different directories never share an EEPROM.
pub fn firmware_dir(data_dir: &Path, quad_path: &Path) -> PathBuf {
    let full = std::fs::canonicalize(quad_path)
        .or_else(|_| std::path::absolute(quad_path))
        .unwrap_or_else(|_| quad_path.to_path_buf());
    let stem = quad_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
    let hash = fnv1a64(full.to_string_lossy().as_bytes()) as u32;
    data_dir.join(format!("{stem}-{hash:08x}"))
}
```

  4. In `build`'s SITL branch, delete the `let quad_stem = …;` line and replace `workdir: opts.data_dir.join(quad_stem),` with `workdir: firmware_dir(&opts.data_dir, &cfg.source_path),`.

In `python/tests/test_sitl_hover.py`:
- replace `{tmp_path}/opendrone-5f-freestyle/sitl.log` with `{tmp_path}/opendrone-5f-freestyle-*/sitl.log`;
- replace `<data_dir>/<quad stem>/eeprom.bin` with `<data_dir>/<quad stem>-<hash>/eeprom.bin`.

- [ ] **Step 7: Run the tests**

Run: `cargo test --workspace && cargo build -p ofs-sim && python -m pytest python/tests -v`
Expected: everything passes (Python 7 passed, 2 skipped); no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates quads python/tests/test_sitl_hover.py
git commit -m "feat(config): radio section (schema 2), per-file firmware dirs, stale-diff guard"
```

---

### Task 8: Wire the radio into the vehicle; Betaflight flies on CRSF

**Files:**
- Replace: `crates/ofs-sim/src/vehicle.rs`
- Modify:
  - `crates/ofs-sim/Cargo.toml`;
  - `crates/ofs-fc/src/sitl/bridge.rs`;
  - `quads/opendrone-5f-freestyle.betaflight.diff`;
  - `crates/ofs-sim/tests/open_loop_vehicle.rs`
- Create: `crates/ofs-sim/tests/sitl_live.rs`

**Interfaces:**
- Consumes:
  - `ElrsLink`, `LinkParams` (Task 3);
  - `SerialLink` (Task 6);
  - `RadioSection`, `firmware_dir` (Task 7).
- Produces, in `ofs_sim::vehicle`:
  - `RadioState { tx_enabled, link_up, lq_pct, rssi_dbm }`;
  - `VehicleState.{radio, fc_restarts}`;
  - `enum Fault { RadioLinkLoss }`;
  - `Vehicle::{set_transmitter(bool), set_fault(Fault, bool), clear_faults(), step_ticks(u64), time_s(), base_hz(), configurator_address() -> Option<String>}`. The last four are used by the server in Task 11.
- **Behaviour:**
  - The transmitter is on after `build`.
  - The ELRS model runs before the FC, so a frame received on a tick reaches Betaflight in that tick's exchange.
  - The bridge sends UDP RC channels as zeros (Decision 2).

- [ ] **Step 1: Write the failing vehicle tests.** In `crates/ofs-sim/tests/open_loop_vehicle.rs`, change the import to `use ofs_sim::vehicle::{build, firmware_dir, BuildOptions, Fault, Sticks, Vehicle};` and append:

```rust

#[test]
fn the_radio_link_is_up_while_the_transmitter_is_on() {
    let mut v = vehicle(1);
    v.run_for(0.5).unwrap();
    let r = v.state().radio;
    assert!(r.tx_enabled && r.link_up, "{r:?}");
    assert_eq!(r.lq_pct, 100.0);
    v.set_transmitter(false);
    v.run_for(0.5).unwrap();
    let r = v.state().radio;
    assert!(!r.tx_enabled && !r.link_up, "{r:?}");
}

#[test]
fn the_link_loss_fault_drops_the_link_until_cleared() {
    let mut v = vehicle(1);
    v.set_fault(Fault::RadioLinkLoss, true);
    v.run_for(0.5).unwrap();
    assert!(!v.state().radio.link_up);
    v.clear_faults();
    v.run_for(0.1).unwrap();
    assert!(v.state().radio.link_up);
}
```

Run: `cargo test -p ofs-sim --test open_loop_vehicle`
Expected: compile errors (`Fault`, `radio`, `set_transmitter`).

- [ ] **Step 2: Implement.** Add `ofs-radio.workspace = true` to `[dependencies]` in `crates/ofs-sim/Cargo.toml`, after `ofs-fc.workspace = true`. Replace `crates/ofs-sim/src/vehicle.rs` with:

```rust
//! Assembles a quad from its config into a scheduler and exposes sticks in, state out.
use std::f64::consts::PI;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use glam::{DQuat, DVec3};
use ofs_config::{FcKind, QuadConfig};
use ofs_core::rng::fnv1a64;
use ofs_core::{names, Bus, Model, Scheduler, Signal, SimError, Wire};
use ofs_electrical::battery::{Battery, BatteryParams};
use ofs_electrical::esc_motor::{EscMotor, EscParams, MotorParams};
use ofs_fc::open_loop::OpenLoopFc;
use ofs_fc::sitl::bridge::{BridgeConfig, SerialLink, SitlBridge};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_fc::sitl::frames::Home;
use ofs_fc::sitl::net;
use ofs_fc::sitl::process::LaunchConfig;
use ofs_physics::propeller::{PropParams, Propeller};
use ofs_physics::rigid_body::{AirframeParams, BodyState, GroundParams, MotorMount, RigidBody};
use ofs_radio::elrs::{ElrsLink, LinkParams};
use ofs_sensors::baro::{Baro, BaroParams};
use ofs_sensors::imu::{Imu, ImuParams};

#[derive(Debug, Clone)]
pub struct BuildOptions {
    pub seed: u64,
    /// Per-quad firmware working directories live under here.
    pub data_dir: PathBuf,
    pub fc_override: Option<FcKind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
    pub aux: [f64; names::RC_AUX_COUNT],
}

impl Default for Sticks {
    fn default() -> Self {
        Self { roll: 0.0, pitch: 0.0, yaw: 0.0, throttle: 0.0, aux: [-1.0; names::RC_AUX_COUNT] }
    }
}

/// What the radio receiver reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadioState {
    /// The transmitter is on (a pilot or script is connected).
    pub tx_enabled: bool,
    pub link_up: bool,
    pub lq_pct: f64,
    pub rssi_dbm: f64,
}

/// Faults a script can inject (spec §6.2). The v1 catalog completes in M4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Every radio uplink packet is lost: the receiver goes silent and Betaflight fails safe.
    RadioLinkLoss,
}

/// The receiver's UART buffer. With no flight controller draining it (open-loop FC), the oldest bytes are
/// dropped like a UART overrun.
const RECEIVER_UART_CAPACITY: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleState {
    pub time_s: f64,
    pub pos_ned_m: DVec3,
    pub vel_ned_mps: DVec3,
    pub att: DQuat,
    pub rate_frd_radps: DVec3,
    pub battery_voltage_v: f64,
    pub battery_current_a: f64,
    pub motor_rpm: Vec<f64>,
    pub motor_cmd: Vec<f64>,
    pub radio: RadioState,
    /// Betaflight reboots so far (e.g. after a Configurator save); always 0 without SITL.
    pub fc_restarts: u32,
}

struct Handles {
    pos: Signal<DVec3>,
    vel: Signal<DVec3>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    vbat: Signal<f64>,
    ibat: Signal<f64>,
    omega: Vec<Signal<f64>>,
    cmd: Vec<Signal<f64>>,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    aux: Vec<Signal<f64>>,
    tx_enabled: Signal<f64>,
    link_up: Signal<f64>,
    lq: Signal<f64>,
    rssi: Signal<f64>,
    fault_radio_loss: Signal<f64>,
    fc_restarts: Signal<f64>,
}

impl Handles {
    fn register(bus: &mut Bus, motors: usize) -> Self {
        Self {
            pos: bus.signal(names::BODY_POS_NED),
            vel: bus.signal(names::BODY_VEL_NED),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            vbat: bus.signal(names::BATTERY_VOLTAGE),
            ibat: bus.signal(names::BATTERY_CURRENT),
            omega: (0..motors).map(|i| bus.signal(&names::motor_omega(i))).collect(),
            cmd: (0..motors).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            aux: (0..names::RC_AUX_COUNT).map(|i| bus.signal(&names::rc_aux(i))).collect(),
            tx_enabled: bus.signal(names::RADIO_TX_ENABLED),
            link_up: bus.signal(names::RADIO_LINK_UP),
            lq: bus.signal(names::RADIO_LQ),
            rssi: bus.signal(names::RADIO_RSSI),
            fault_radio_loss: bus.signal(names::FAULT_RADIO_LINK_LOSS),
            fc_restarts: bus.signal(names::FC_RESTARTS),
        }
    }
}

pub struct Vehicle {
    scheduler: Scheduler,
    h: Handles,
    sitl: bool,
}

/// Per-quad firmware directory: `<data_dir>/<quad file stem>-<hash of the quad file's path>`, so quads with
/// the same file name in different directories never share an EEPROM.
pub fn firmware_dir(data_dir: &Path, quad_path: &Path) -> PathBuf {
    let full = std::fs::canonicalize(quad_path)
        .or_else(|_| std::path::absolute(quad_path))
        .unwrap_or_else(|_| quad_path.to_path_buf());
    let stem = quad_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "quad".into());
    let hash = fnv1a64(full.to_string_lossy().as_bytes()) as u32;
    data_dir.join(format!("{stem}-{hash:08x}"))
}

fn v3(a: [f64; 3]) -> DVec3 {
    DVec3::from_array(a)
}

fn pairs(t: &[[f64; 2]]) -> Vec<(f64, f64)> {
    t.iter().map(|r| (r[0], r[1])).collect()
}

pub fn build(cfg: &QuadConfig, opts: &BuildOptions) -> Result<Vehicle, SimError> {
    let mut bus = Bus::new();
    let n = cfg.frame.motor_positions_frd_m.len();
    let base_hz = cfg.sim.base_hz;
    let mut models: Vec<Box<dyn Model>> = Vec::new();

    let prop = PropParams { diameter_m: cfg.prop.diameter_m, ct_table: pairs(&cfg.prop.ct_table), cp_table: pairs(&cfg.prop.cp_table) };
    for i in 0..n {
        models.push(Box::new(Propeller::new(i, prop.clone(), &mut bus)));
    }

    let motor = MotorParams {
        kv_rpm_per_v: cfg.motor.kv_rpm_per_v,
        resistance_ohm: cfg.motor.resistance_ohm,
        no_load_current_a: cfg.motor.no_load_current_a,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2,
    };
    let esc = EscParams { response_tau_s: cfg.esc.response_tau_s, current_limit_a: cfg.esc.current_limit_a };
    for i in 0..n {
        models.push(Box::new(EscMotor::new(i, esc.clone(), motor.clone(), cfg.prop.inertia_kgm2, &mut bus)));
    }

    let battery = BatteryParams {
        cells: cfg.battery.cells,
        capacity_mah: cfg.battery.capacity_mah,
        r0_ohm: cfg.battery.r0_ohm,
        r1_ohm: cfg.battery.r1_ohm,
        c1_f: cfg.battery.c1_f,
        ocv_table: pairs(&cfg.battery.ocv_table),
        initial_soc: cfg.battery.initial_soc,
    };
    models.push(Box::new(Battery::new(battery, n, base_hz / cfg.battery.rate_hz, &mut bus)));

    let f = &cfg.frame;
    let airframe = AirframeParams {
        mass_kg: f.mass_kg,
        inertia_kgm2: v3(f.inertia_kgm2),
        cda_m2: v3(f.cda_m2),
        angular_damping_nms: f.angular_damping_nms,
        rotor_inertia_kgm2: cfg.motor.rotor_inertia_kgm2 + cfg.prop.inertia_kgm2,
        mounts: f
            .motor_positions_frd_m
            .iter()
            .zip(&f.motor_spin)
            .map(|(p, s)| MotorMount { position_frd_m: v3(*p), spin: f64::from(*s) })
            .collect(),
        contact_points_frd_m: f.contact_points_frd_m.iter().map(|p| v3(*p)).collect(),
        ground: GroundParams {
            stiffness_npm: cfg.ground.stiffness_npm,
            damping_nspm: cfg.ground.damping_nspm,
            friction_coeff: cfg.ground.friction_coeff,
        },
    };
    let initial = BodyState {
        pos_ned_m: v3(cfg.initial.position_ned_m),
        vel_ned_mps: DVec3::ZERO,
        att: DQuat::from_rotation_z(cfg.initial.yaw_deg.to_radians()),
        rate_frd_radps: DVec3::ZERO,
    };
    models.push(Box::new(RigidBody::new(airframe, initial, &mut bus)));

    let imu = ImuParams {
        gyro_noise_std_radps: cfg.imu.gyro_noise_std_radps,
        gyro_bias_radps: v3(cfg.imu.gyro_bias_radps),
        accel_noise_std_mps2: cfg.imu.accel_noise_std_mps2,
        accel_bias_mps2: v3(cfg.imu.accel_bias_mps2),
    };
    models.push(Box::new(Imu::new(imu, opts.seed, &mut bus)));
    models.push(Box::new(Baro::new(BaroParams { noise_std_pa: cfg.baro.noise_std_pa, home_alt_m: cfg.home.alt_m }, opts.seed, &mut bus)));

    let r = &cfg.radio;
    let link = LinkParams {
        packet_rate_hz: r.packet_rate_hz,
        latency_packets: r.latency_packets,
        loss_good: r.loss_good,
        loss_bad: r.loss_bad,
        p_good_to_bad: r.p_good_to_bad,
        p_bad_to_good: r.p_bad_to_good,
        rssi_dbm: r.rssi_dbm,
        snr_db: r.snr_db,
        link_stats_interval_packets: r.link_stats_interval_packets,
        rf_mode: r.rf_mode,
        tx_power: r.tx_power,
    };
    let receiver_uart = Wire::new(RECEIVER_UART_CAPACITY);
    // Before the FC: a frame received on a tick reaches Betaflight in that tick's exchange.
    models.push(Box::new(ElrsLink::new(link, base_hz / r.packet_rate_hz, opts.seed, receiver_uart.clone(), &mut bus)));

    let fc_divisor = base_hz / cfg.fc.exchange_hz;
    let fc_kind = opts.fc_override.unwrap_or(cfg.fc.kind);
    match fc_kind {
        FcKind::OpenLoop => models.push(Box::new(OpenLoopFc::new(n, fc_divisor, &mut bus))),
        FcKind::Sitl => {
            let launch_argv = env_argv("OFS_SITL_LAUNCH").unwrap_or_else(|| cfg.fc.launch.clone());
            let mut cleanup = env_argv("OFS_SITL_CLEANUP").unwrap_or_else(|| cfg.fc.cleanup.clone());
            if cleanup.is_empty() {
                cleanup = net::default_cleanup(&launch_argv); // required under WSL (M0 §6)
            }
            let sitl_net = net::resolve(&launch_argv, env_ip("OFS_SITL_HOST")?, env_ip("OFS_SITL_REPLY_IP")?)
                .map_err(|e| SimError::Firmware(e.to_string()))?;
            let launch = LaunchConfig {
                launch: launch_argv,
                cleanup,
                workdir: firmware_dir(&opts.data_dir, &cfg.source_path),
                diff_file: cfg.resolve(&cfg.fc.betaflight_diff),
                startup_timeout: Duration::from_millis(cfg.fc.startup_timeout_ms),
            };
            let bridge = SitlBridge::start(
                BridgeConfig {
                    launch,
                    net: sitl_net,
                    rate_divisor: fc_divisor,
                    first_reply_timeout: Duration::from_millis(cfg.fc.first_reply_timeout_ms),
                    reply_timeout: Duration::from_millis(cfg.fc.reply_timeout_ms),
                    home: Home { lat_deg: cfg.home.lat_deg, lon_deg: cfg.home.lon_deg, alt_m: cfg.home.alt_m },
                    motor_count: n,
                    serial: vec![SerialLink { uart_index: r.uart - 1, rx: receiver_uart }],
                },
                &mut bus,
            )
            .map_err(|e| SimError::Firmware(e.to_string()))?;
            models.push(Box::new(bridge));
        }
    }

    let h = Handles::register(&mut bus, n);
    let mut scheduler = Scheduler::new(base_hz, bus);
    for m in models {
        scheduler.add(m);
    }
    let mut vehicle = Vehicle { scheduler, h, sitl: fc_kind == FcKind::Sitl };
    // The bus starts every signal at zero; aux 0.0 would reach Betaflight as 1500 us until the first SetSticks.
    vehicle.set_sticks(&Sticks::default());
    vehicle.set_transmitter(true);
    Ok(vehicle)
}

impl Vehicle {
    pub fn set_sticks(&mut self, s: &Sticks) {
        let h = &self.h;
        let bus = self.scheduler.bus_mut();
        bus.set(h.roll, s.roll);
        bus.set(h.pitch, s.pitch);
        bus.set(h.yaw, s.yaw);
        bus.set(h.throttle, s.throttle);
        for (sig, v) in h.aux.iter().zip(s.aux) {
            bus.set(*sig, v);
        }
    }

    /// Transmitter on (pilot or script connected) or off (the receiver hears nothing).
    pub fn set_transmitter(&mut self, on: bool) {
        let s = self.h.tx_enabled;
        self.scheduler.bus_mut().set(s, if on { 1.0 } else { 0.0 });
    }

    pub fn set_fault(&mut self, fault: Fault, active: bool) {
        let s = match fault {
            Fault::RadioLinkLoss => self.h.fault_radio_loss,
        };
        self.scheduler.bus_mut().set(s, if active { 1.0 } else { 0.0 });
    }

    pub fn clear_faults(&mut self) {
        self.set_fault(Fault::RadioLinkLoss, false);
    }

    pub fn run_for(&mut self, seconds: f64) -> Result<(), SimError> {
        self.scheduler.run_for(seconds)
    }

    /// Steps `ticks` base ticks. On error the failing tick is not counted.
    pub fn step_ticks(&mut self, ticks: u64) -> Result<(), SimError> {
        for _ in 0..ticks {
            self.scheduler.step()?;
        }
        Ok(())
    }

    pub fn time_s(&self) -> f64 {
        self.scheduler.time_s()
    }

    pub fn base_hz(&self) -> u32 {
        self.scheduler.base_hz()
    }

    /// Where Betaflight Configurator can connect (SITL's MSP UART), when this vehicle runs Betaflight SITL.
    pub fn configurator_address(&self) -> Option<String> {
        self.sitl.then(|| format!("tcp://127.0.0.1:{MSP_TCP_PORT}"))
    }

    pub fn state(&self) -> VehicleState {
        let b = self.scheduler.bus();
        let h = &self.h;
        VehicleState {
            time_s: self.scheduler.time_s(),
            pos_ned_m: b.get(h.pos),
            vel_ned_mps: b.get(h.vel),
            att: b.get(h.att),
            rate_frd_radps: b.get(h.rate),
            battery_voltage_v: b.get(h.vbat),
            battery_current_a: b.get(h.ibat),
            motor_rpm: h.omega.iter().map(|s| b.get(*s) * 60.0 / (2.0 * PI)).collect(),
            motor_cmd: h.cmd.iter().map(|s| b.get(*s)).collect(),
            radio: RadioState {
                tx_enabled: b.get(h.tx_enabled) > 0.5,
                link_up: b.get(h.link_up) > 0.5,
                lq_pct: b.get(h.lq),
                rssi_dbm: b.get(h.rssi),
            },
            fc_restarts: b.get(h.fc_restarts) as u32,
        }
    }

    pub fn digest(&self) -> u64 {
        self.scheduler.bus().digest()
    }
}

/// Space-separated argv from an environment variable, if set and non-empty.
fn env_argv(var: &str) -> Option<Vec<String>> {
    std::env::var(var).ok().filter(|s| !s.trim().is_empty()).map(|s| s.split_whitespace().map(String::from).collect())
}

/// IPv4 address from an environment variable, if set and non-empty.
fn env_ip(var: &str) -> Result<Option<Ipv4Addr>, SimError> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse()
            .map(Some)
            .map_err(|_| SimError::InvalidArgument(format!("{var}={v} is not an IPv4 address"))),
        _ => Ok(None),
    }
}
```

In `crates/ofs-fc/src/sitl/bridge.rs`:
1. In `struct Inputs`, delete the fields `roll`, `pitch`, `yaw`, `throttle`, `aux`, and their initialisers in `start`.
2. In `step`, replace the two lines from `let aux: Vec<f64> = …` to `let rc = RcPacket { … channels };` with:

```rust
        // Pilot input reaches Betaflight only as CRSF on its receiver UART (spec §1.5). The UDP RC channels are
        // zero: invalid pulses, so a SITL whose EEPROM still selects the UDP receiver fails safe instead of flying.
        let rc = RcPacket { timestamp_s: ctx.time_s, channels: [0; 16] };
```

3. Remove `rc_channels` from the `use super::frames::{…}` import. The function stays in `frames.rs`, which its own tests still cover.

In `quads/opendrone-5f-freestyle.betaflight.diff`, replace the line `feature -GPS` with:

```
feature -GPS
# CRSF receiver on UART2 (serial index 1): the simulated ExpressLRS receiver is wired there.
feature RX_SERIAL
serial 1 64 115200 57600 0 115200
set serialrx_provider = CRSF
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --workspace`
Expected: all pass, no warnings. The determinism test `same_seed_and_inputs_give_identical_runs` still passes; the radio draws from its own seeded stream.

- [ ] **Step 4: Live M2 checks against Betaflight.** Create `crates/ofs-sim/tests/sitl_live.rs`:

```rust
//! M2 exit checks against real Betaflight SITL (spec §8.3). Needs a built SITL; run one at a time:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-sim --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ofs_config::load;
use ofs_fc::msp::{api_version, rc_channels_us, MspClient, MSP_API_VERSION, MSP_RC};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
/// AUX1 high = ARM, AUX2 high = ANGLE (the quad's betaflight.diff).
const ARM_AND_ANGLE: [f64; 4] = [1.0, 1.0, -1.0, -1.0];

/// A quad flying Betaflight SITL from a fresh firmware directory. Fields drop in order: SITL stops before
/// its directory is removed.
struct Rig {
    v: Vehicle,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(Path::new(QUAD)).unwrap();
        let v = build(&cfg, &BuildOptions { seed: 1, data_dir: dir.path().to_path_buf(), fc_override: None }).unwrap();
        Self { v, _dir: dir }
    }
}

fn msp() -> MspClient {
    MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap()
}

#[test]
#[ignore]
fn msp_reports_the_api_version_over_tcp() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(1.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    let (protocol, major, minor) = api_version(&reply).unwrap();
    assert_eq!((protocol, major), (0, 1), "MSP API {major}.{minor}");
}

#[test]
#[ignore]
fn crsf_sticks_reach_betaflight() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.set_sticks(&Sticks { roll: 0.2, pitch: -0.2, ..Sticks::default() });
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_RC, &[], 500, || v.run_for(0.01)).unwrap();
    // MSP_RC lists roll, pitch, yaw, throttle (Betaflight's internal order).
    assert_eq!(&rc_channels_us(&reply)[..4], &[1600, 1400, 1500, 1000]);
}

fn armed(v: &Vehicle) -> bool {
    v.state().motor_cmd.iter().all(|m| *m > 0.0)
}

#[test]
#[ignore]
fn a_radio_cut_fails_safe_on_betaflight_timing() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(4.0).unwrap(); // boot and gyro calibration
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() });
    v.run_for(1.0).unwrap();
    assert!(armed(v), "never armed: {:?}", v.state().motor_cmd);
    v.set_fault(Fault::RadioLinkLoss, true);
    let cut = v.state().time_s;
    let mut disarmed_after = None;
    while v.state().time_s < cut + 4.0 {
        v.run_for(0.01).unwrap();
        if v.state().motor_cmd.iter().all(|m| *m == 0.0) {
            disarmed_after = Some(v.state().time_s - cut);
            break;
        }
    }
    let dt = disarmed_after.expect("Betaflight never disarmed after the radio cut");
    // Betaflight declares RX loss once frames stop for failsafe_delay (1.5 s by default) and then disarms
    // (src/main/flight/failsafe.c); stage-1 failsafe holds idle until then.
    assert!((1.4..=2.2).contains(&dt), "disarmed {dt:.3} s after the cut");
}
```

Run with `OFS_SITL_LAUNCH` set, 3 times:

```bash
cargo test -p ofs-sim --test sitl_live -- --ignored --test-threads=1
cargo build -p ofs-sim && python -m pytest python/tests/test_sitl_hover.py -v
```

Expected:
- 3 passed every time;
- the failsafe disarm lands within 1.4–2.2 s of the cut;
- the hover and its determinism test pass. They now fly through ELRS/CRSF.

Then run the same `sitl_live` command natively on Linux.

- [ ] **Step 5: Commit**

```bash
git add crates quads
git commit -m "feat(sim): fly Betaflight through the ExpressLRS/CRSF link; failsafe and MSP checks"
```

---

### Task 9: Betaflight reboots are firmware restarts

**Files:**
- Replace: `crates/ofs-fc/src/sitl/process.rs`, `crates/ofs-fc/src/sitl/bridge.rs`, `crates/ofs-sim/tests/sitl_live.rs`
- Modify: `crates/ofs-fc/tests/sitl_errors.rs`

**Interfaces:**
- Produces:
  - `process::{is_reset_line, REBOOT_GRACE = 3 s}` and `SitlProcess::rebooted(&mut self) -> bool`;
  - the bridge relaunches SITL from the same `LaunchConfig` (with `--ip` under WSL) and increments `fc.restarts` (`names::FC_RESTARTS`).
- **Why the 3 s grace:** before resetting, Betaflight's `motorShutdown()` sleeps 501.5 ms of real time (PWM ESCs), longer than the 500 ms per-exchange timeout. SITL then joins its threads before exiting.
- **The old process is dropped before the new one starts:** its cleanup command (`pkill -x betaflight_SITL`) would otherwise kill the new instance.

- [ ] **Step 1: Write the failing tests.** Append to `crates/ofs-fc/tests/sitl_errors.rs`:

```rust

#[test]
fn reset_lines_are_recognised() {
    // Betaflight's systemReset() / systemResetToBootloader() in SITL, just before exit(0).
    assert!(is_reset_line("[system]Reset!"));
    assert!(is_reset_line("[system]ResetToBootloader!"));
    assert!(!is_reset_line("[system]Init..."));
}
```

and change the import to `use ofs_fc::sitl::process::{is_bind_failure, is_ready_line, is_reset_line, LaunchConfig};`. Replace `crates/ofs-sim/tests/sitl_live.rs` with the version that adds the reboot test:

```rust
//! M2 exit checks against real Betaflight SITL (spec §8.3). Needs a built SITL; run one at a time:
//!   OFS_SITL_LAUNCH="<path or wsl.exe -e path>" cargo test -p ofs-sim --test sitl_live -- --ignored --test-threads=1
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ofs_config::load;
use ofs_fc::msp::{api_version, rc_channels_us, MspClient, MSP_API_VERSION, MSP_RC, MSP_REBOOT};
use ofs_fc::sitl::codec::MSP_TCP_PORT;
use ofs_sim::vehicle::{build, BuildOptions, Fault, Sticks, Vehicle};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");
/// AUX1 high = ARM, AUX2 high = ANGLE (the quad's betaflight.diff).
const ARM_AND_ANGLE: [f64; 4] = [1.0, 1.0, -1.0, -1.0];

/// A quad flying Betaflight SITL from a fresh firmware directory. Fields drop in order: SITL stops before
/// its directory is removed.
struct Rig {
    v: Vehicle,
    _dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        assert!(std::env::var("OFS_SITL_LAUNCH").is_ok(), "set OFS_SITL_LAUNCH");
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(Path::new(QUAD)).unwrap();
        let v = build(&cfg, &BuildOptions { seed: 1, data_dir: dir.path().to_path_buf(), fc_override: None }).unwrap();
        Self { v, _dir: dir }
    }
}

fn msp() -> MspClient {
    MspClient::connect(SocketAddr::from(([127, 0, 0, 1], MSP_TCP_PORT)), Duration::from_secs(5)).unwrap()
}

#[test]
#[ignore]
fn msp_reports_the_api_version_over_tcp() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(1.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    let (protocol, major, minor) = api_version(&reply).unwrap();
    assert_eq!((protocol, major), (0, 1), "MSP API {major}.{minor}");
}

#[test]
#[ignore]
fn crsf_sticks_reach_betaflight() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.set_sticks(&Sticks { roll: 0.2, pitch: -0.2, ..Sticks::default() });
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_RC, &[], 500, || v.run_for(0.01)).unwrap();
    // MSP_RC lists roll, pitch, yaw, throttle (Betaflight's internal order).
    assert_eq!(&rc_channels_us(&reply)[..4], &[1600, 1400, 1500, 1000]);
}

fn armed(v: &Vehicle) -> bool {
    v.state().motor_cmd.iter().all(|m| *m > 0.0)
}

#[test]
#[ignore]
fn a_radio_cut_fails_safe_on_betaflight_timing() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(4.0).unwrap(); // boot and gyro calibration
    v.set_sticks(&Sticks { aux: ARM_AND_ANGLE, ..Sticks::default() });
    v.run_for(1.0).unwrap();
    assert!(armed(v), "never armed: {:?}", v.state().motor_cmd);
    v.set_fault(Fault::RadioLinkLoss, true);
    let cut = v.state().time_s;
    let mut disarmed_after = None;
    while v.state().time_s < cut + 4.0 {
        v.run_for(0.01).unwrap();
        if v.state().motor_cmd.iter().all(|m| *m == 0.0) {
            disarmed_after = Some(v.state().time_s - cut);
            break;
        }
    }
    let dt = disarmed_after.expect("Betaflight never disarmed after the radio cut");
    // Betaflight declares RX loss once frames stop for failsafe_delay (1.5 s by default) and then disarms
    // (src/main/flight/failsafe.c); stage-1 failsafe holds idle until then.
    assert!((1.4..=2.2).contains(&dt), "disarmed {dt:.3} s after the cut");
}

#[test]
#[ignore]
fn a_betaflight_reboot_is_a_firmware_restart_not_a_crash() {
    let mut rig = Rig::new();
    let v = &mut rig.v;
    v.run_for(2.0).unwrap();
    // Betaflight resets after MSP_REBOOT: SITL prints "[system]Reset!" and exits with status 0. (Its reply can
    // be lost with the connection, so the test only sends the command.)
    let mut client = msp();
    client.send(MSP_REBOOT, &[]).unwrap();
    let give_up = v.state().time_s + 5.0;
    while v.state().fc_restarts == 0 && v.state().time_s < give_up {
        v.run_for(0.01).unwrap();
    }
    assert_eq!(v.state().fc_restarts, 1);
    v.run_for(2.0).unwrap();
    let reply = msp().request(MSP_API_VERSION, &[], 500, || v.run_for(0.01)).unwrap();
    assert_eq!(api_version(&reply).map(|(_, major, _)| major), Some(1));
}
```

Run: `cargo test -p ofs-fc --test sitl_errors`
Expected: compile error, no `is_reset_line`.

- [ ] **Step 2: Implement.** Replace `crates/ofs-fc/src/sitl/process.rs` with the full file below. It keeps Tasks 5 and 7 and adds the reset tracking:

```rust
//! Launches and supervises the Betaflight SITL process; applies the CLI diff on first boot.
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::FcError;

const TAIL_LINES: usize = 200;
/// How long a missed reply waits for a Betaflight reboot to show itself (see [`SitlProcess::rebooted`]).
pub const REBOOT_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// argv; on Windows typically `wsl.exe -e /home/<user>/.../betaflight_SITL.elf`.
    pub launch: Vec<String>,
    /// argv run before launch and after stop to remove stray processes; may be empty.
    pub cleanup: Vec<String>,
    /// SITL's working directory: holds eeprom.bin, betaflight.diff and sitl.log.
    pub workdir: PathBuf,
    pub diff_file: PathBuf,
    pub startup_timeout: Duration,
}

/// A stale SITL still holds the ports: a new instance prints this (e.g. `bind port 5761 for UART1 failed!!`)
/// and keeps running, and the simulator would talk to the old one. Treated as a startup error.
pub fn is_bind_failure(line: &str) -> bool {
    line.contains("bind port") && line.contains("failed")
}

/// The patched SITL prints this when init has finished and its scheduler runs; only then does it accept
/// state packets (third_party/betaflight/ofs-sitl.patch: deterministic boot).
pub const READY_LINE: &str = "[SITL] ready for the simulator";

pub fn is_ready_line(line: &str) -> bool {
    line.contains(READY_LINE)
}

/// Betaflight's `systemReset()` (a reboot, e.g. after a Configurator save) prints this before `exit(0)`;
/// so does the reset to bootloader. Together with a clean exit it means "restart me", not a crash.
pub fn is_reset_line(line: &str) -> bool {
    line.contains("[system]Reset")
}

#[derive(Default)]
struct LogSink {
    tail: VecDeque<String>,
    file: Option<File>,
    bind_failed: bool,
    ready_seen: bool,
    reset_seen: bool,
}

impl LogSink {
    fn push(&mut self, line: String) {
        if is_bind_failure(&line) {
            self.bind_failed = true;
        }
        if is_ready_line(&line) {
            self.ready_seen = true;
        }
        if is_reset_line(&line) {
            self.reset_seen = true;
        }
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        if self.tail.len() == TAIL_LINES {
            self.tail.pop_front();
        }
        self.tail.push_back(line);
    }
}

type SharedLog = Arc<Mutex<LogSink>>;

pub struct SitlProcess {
    child: Child,
    log: SharedLog,
    cleanup: Vec<String>,
}

impl SitlProcess {
    pub fn start(cfg: &LaunchConfig) -> Result<Self, FcError> {
        std::fs::create_dir_all(&cfg.workdir)?;
        let workdir = std::path::absolute(&cfg.workdir)?;
        if workdir.join("eeprom.bin").exists() {
            check_diff_unchanged(cfg, &workdir)?;
        }
        run_cleanup(&cfg.cleanup);
        if !workdir.join("eeprom.bin").exists() {
            apply_diff(cfg, &workdir)?;
        }
        let log: SharedLog = Arc::new(Mutex::new(LogSink { file: File::create(workdir.join("sitl.log")).ok(), ..LogSink::default() }));
        let child = spawn_logged(&cfg.launch, &workdir, &log)?;
        let mut proc = SitlProcess { child, log, cleanup: cfg.cleanup.clone() };
        proc.wait_until_ready(cfg.startup_timeout)?;
        Ok(proc)
    }

    /// Waits for SITL to announce it is ready ([`READY_LINE`]). A stale instance holding the ports makes the new one
    /// print `bind port ... failed` during init, before that line: a startup error.
    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), FcError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.exit_status() {
                return Err(FcError::Startup(format!("exited with {status} during startup"), self.log_tail()));
            }
            let (ready, bind_failed) = self.log.lock().map(|l| (l.ready_seen, l.bind_failed)).unwrap_or((false, false));
            if bind_failed {
                let what = "could not bind its ports (a stale SITL is probably still running; see fc.cleanup / OFS_SITL_CLEANUP)";
                return Err(FcError::Startup(what.into(), self.log_tail()));
            }
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let what = format!(
                    "did not print \"{READY_LINE}\" within {} ms (is it the lockstep build from scripts/build-sitl.sh?)",
                    timeout.as_millis()
                );
                return Err(FcError::Startup(what, self.log_tail()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn exit_status(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// True when SITL announced a reset ([`is_reset_line`]) and exited cleanly: Betaflight rebooted.
    ///
    /// Called after a missed reply, so it waits up to [`REBOOT_GRACE`] for both: before resetting, Betaflight's
    /// `motorShutdown()` sleeps 0.5 s of real time (PWM ESCs), longer than the per-exchange reply timeout, and
    /// SITL then joins its threads before exiting. A SITL that has merely hung costs this grace once.
    pub fn rebooted(&mut self) -> bool {
        let deadline = Instant::now() + REBOOT_GRACE;
        loop {
            let reset_seen = self.log.lock().map(|l| l.reset_seen).unwrap_or(false);
            match self.exit_status() {
                Some(status) if reset_seen => return status.success(),
                _ if Instant::now() >= deadline => return false,
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    pub fn log_tail(&self) -> String {
        self.log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default()
    }
}

impl Drop for SitlProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        run_cleanup(&self.cleanup);
    }
}

fn command(argv: &[String], workdir: &Path) -> Result<Command, FcError> {
    let (program, args) = argv.split_first().ok_or_else(|| FcError::Config("fc.launch is empty".into()))?;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(workdir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    Ok(cmd)
}

fn spawn_logged(argv: &[String], workdir: &Path, log: &SharedLog) -> Result<Child, FcError> {
    let mut child = command(argv, workdir)?
        .spawn()
        .map_err(|source| FcError::Launch { command: argv.join(" "), source })?;
    pipe_lines(child.stdout.take().expect("stdout is piped"), log.clone());
    pipe_lines(child.stderr.take().expect("stderr is piped"), log.clone());
    Ok(child)
}

/// The quad's diff is applied on first boot only, so an EEPROM made from an older diff no longer matches the
/// quad file. Refuse to start rather than fly a stale configuration.
fn check_diff_unchanged(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    let Ok(applied) = std::fs::read(workdir.join("betaflight.diff")) else {
        return Ok(()); // first booted before copies of the applied diff were kept
    };
    if applied != std::fs::read(&cfg.diff_file)? {
        return Err(FcError::Config(format!(
            "{} changed since this quad's first boot; delete {} to apply it again (this also discards settings              changed in Betaflight Configurator)",
            cfg.diff_file.display(),
            workdir.join("eeprom.bin").display()
        )));
    }
    Ok(())
}

/// First boot: `<launch> --config betaflight.diff` loads the diff, saves eeprom.bin and exits.
fn apply_diff(cfg: &LaunchConfig, workdir: &Path) -> Result<(), FcError> {
    std::fs::copy(&cfg.diff_file, workdir.join("betaflight.diff"))?;
    let mut argv = cfg.launch.clone();
    argv.extend(["--config".to_string(), "betaflight.diff".to_string()]);
    let log: SharedLog = Arc::new(Mutex::new(LogSink::default()));
    let mut child = spawn_logged(&argv, workdir, &log)?;
    let deadline = Instant::now() + cfg.startup_timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FcError::Config(format!("`{}` did not exit within {} ms", argv.join(" "), cfg.startup_timeout.as_millis())));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if !workdir.join("eeprom.bin").exists() {
        let tail = log.lock().map(|l| l.tail.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default();
        return Err(FcError::Config(format!("`{}` exited with {status} without writing eeprom.bin; output:\n{tail}", argv.join(" "))));
    }
    Ok(())
}

fn pipe_lines(stream: impl Read + Send + 'static, log: SharedLog) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    tracing::debug!(target: "sitl", "{line}");
                    if let Ok(mut l) = log.lock() {
                        l.push(line);
                    }
                }
            }
        }
    });
}

pub(crate) fn run_cleanup(argv: &[String]) {
    if let Some((program, args)) = argv.split_first() {
        let _ = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}
```

Replace `crates/ofs-fc/src/sitl/bridge.rs` with the full file below. It keeps Tasks 6 and 8, and moves the exchange into `exchange()` so it can be retried after a restart:

```rust
//! Lockstep exchange with Betaflight SITL: send one state datagram (sensors, plus bytes for SITL's UARTs),
//! wait for one motor packet. A Betaflight reboot (e.g. a Configurator save) relaunches SITL.
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx, Wire};

use super::codec::{
    state_datagram_with_serial, RcPacket, ServoPacket, PORT_PWM, PORT_STATE, SERIAL_BLOCK_HEADER, SERIAL_SECTION_MAX,
};
use super::frames::{fdm_packet, motor_commands, Home, SensorFrame};
use super::net::SitlNet;
use super::process::{run_cleanup, LaunchConfig, SitlProcess};
use super::FcError;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub launch: LaunchConfig,
    pub net: SitlNet,
    pub rate_divisor: u32,
    /// The first exchange can arrive before SITL's main loop runs; allow seconds. The first datagram is
    /// resent every [`FIRST_RESEND_INTERVAL`] until SITL answers or this elapses.
    pub first_reply_timeout: Duration,
    pub reply_timeout: Duration,
    pub home: Home,
    pub motor_count: usize,
    /// Bytes for SITL's UARTs, carried in the state datagram (e.g. the receiver's CRSF into UART2).
    pub serial: Vec<SerialLink>,
}

/// Bytes the simulator feeds into one of SITL's UARTs.
#[derive(Debug, Clone)]
pub struct SerialLink {
    /// 0-based: UART2 = 1.
    pub uart_index: u8,
    pub rx: Wire,
}

struct Inputs {
    gyro: Signal<DVec3>,
    accel: Signal<DVec3>,
    att: Signal<DQuat>,
    vel: Signal<DVec3>,
    pos: Signal<DVec3>,
    pressure: Signal<f64>,
}

pub struct SitlBridge {
    cfg: BridgeConfig,
    /// What was launched (with `--ip` under WSL); launched again when Betaflight reboots.
    launch: LaunchConfig,
    rx: UdpSocket,
    tx: UdpSocket,
    inputs: Inputs,
    cmds: Vec<Signal<f64>>,
    restarts: Signal<f64>,
    proc: Option<SitlProcess>,
    answered: bool,
}

/// How often the first state datagram is resent while SITL has not answered it.
pub const FIRST_RESEND_INTERVAL: Duration = Duration::from_millis(250);

impl SitlBridge {
    pub fn start(cfg: BridgeConfig, bus: &mut Bus) -> Result<Self, FcError> {
        assert!(cfg.motor_count <= 4, "Betaflight SITL's servo_packet carries 4 motors");
        let net = cfg.net;
        let rx = UdpSocket::bind(SocketAddr::from((net.bind_ip, PORT_PWM)))
            .map_err(|_| FcError::PortInUse { port: PORT_PWM, hint: "another simulator instance may be running" })?;
        if net.send_ip.is_loopback() {
            // Native SITL must be able to bind its state port; probe and release it to fail early.
            // (Under WSL the port lives inside the VM; the cleanup command and bind-failure check cover it.)
            probe_state_port(net.send_ip, &cfg.launch.cleanup)?;
        }
        let tx = UdpSocket::bind(SocketAddr::from((net.bind_ip, 0)))?;
        let inputs = Inputs {
            gyro: bus.signal(names::IMU_GYRO),
            accel: bus.signal(names::IMU_ACCEL),
            att: bus.signal(names::BODY_ATT),
            vel: bus.signal(names::BODY_VEL_NED),
            pos: bus.signal(names::BODY_POS_NED),
            pressure: bus.signal(names::BARO_PRESSURE),
        };
        let cmds = (0..cfg.motor_count).map(|i| bus.signal(&names::motor_cmd(i))).collect();
        let restarts = bus.signal(names::FC_RESTARTS);
        let mut launch = cfg.launch.clone();
        if let Some(ip) = net.sitl_ip_arg {
            launch.launch.extend(["--ip".to_string(), ip.to_string()]);
        }
        let proc = SitlProcess::start(&launch)?;
        Ok(Self { cfg, launch, rx, tx, inputs, cmds, restarts, proc: Some(proc), answered: false })
    }

    fn drain(&self) -> std::io::Result<()> {
        self.rx.set_nonblocking(true)?;
        let mut buf = [0u8; 128];
        while self.rx.recv_from(&mut buf).is_ok() {}
        self.rx.set_nonblocking(false)
    }

    fn firmware_error(&mut self, what: &str) -> SimError {
        let Some(proc) = self.proc.as_mut() else {
            return SimError::Firmware(format!("{what} (SITL is not running)"));
        };
        let state = match proc.exit_status() {
            Some(status) => format!("SITL exited with {status}"),
            None => format!("{what} (no reply: datagram lost or SITL hung)"),
        };
        SimError::Firmware(format!("{state}; last output:\n{}", proc.log_tail()))
    }

    /// Takes pending UART bytes for this datagram, within SITL's serial section limit.
    fn serial_blocks(&self) -> Vec<(u8, Vec<u8>)> {
        let mut blocks = Vec::new();
        let mut room = SERIAL_SECTION_MAX;
        for link in &self.cfg.serial {
            if room <= SERIAL_BLOCK_HEADER {
                break;
            }
            let bytes = link.rx.take(room - SERIAL_BLOCK_HEADER);
            if !bytes.is_empty() {
                room -= SERIAL_BLOCK_HEADER + bytes.len();
                blocks.push((link.uart_index, bytes));
            }
        }
        blocks
    }

    /// One send/receive. Until SITL has answered once, the datagram is resent (see [`exchange_with_resend`]).
    fn exchange(&mut self, datagram: &[u8], buf: &mut [u8]) -> std::io::Result<usize> {
        let to = SocketAddr::from((self.cfg.net.send_ip, PORT_STATE));
        if self.answered {
            send(&self.tx, datagram, to);
            return self.rx.recv_from(buf).map(|(n, _)| n);
        }
        let timeout = self.cfg.first_reply_timeout;
        let (n, resends) = exchange_with_resend(&self.rx, &self.tx, to, datagram, buf, timeout, FIRST_RESEND_INTERVAL)?;
        if resends > 0 {
            tracing::warn!(
                "SITL did not answer the first state datagram; resent it {resends} time(s). SITL may have run an \
                 extra tick at this instant, so firmware determinism is only claimed for runs without a resend"
            );
        }
        self.answered = true;
        self.rx.set_read_timeout(Some(self.cfg.reply_timeout))?;
        Ok(n)
    }

    fn rebooted(&mut self) -> bool {
        self.proc.as_mut().is_some_and(|p| p.rebooted())
    }

    /// Betaflight rebooted: launch SITL again from its EEPROM. SITL's clock restarts from the next packet.
    fn restart(&mut self) -> Result<(), SimError> {
        // Drop the old process first: its cleanup command would kill the new instance.
        self.proc = None;
        let proc = SitlProcess::start(&self.launch)
            .map_err(|e| SimError::Firmware(format!("relaunching Betaflight SITL after a reboot failed: {e}")))?;
        self.proc = Some(proc);
        self.answered = false;
        Ok(())
    }
}

/// Checks that SITL's state port is free. If it is held (typically by a SITL orphaned by a killed
/// simulator), runs the cleanup command and waits briefly for the port to be released.
fn probe_state_port(ip: Ipv4Addr, cleanup: &[String]) -> Result<(), FcError> {
    let free = || UdpSocket::bind(SocketAddr::from((ip, PORT_STATE))).is_ok();
    if free() {
        return Ok(());
    }
    if !cleanup.is_empty() {
        run_cleanup(cleanup);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            if free() {
                return Ok(());
            }
        }
    }
    Err(FcError::PortInUse {
        port: PORT_STATE,
        hint: "a Betaflight SITL instance may still be running (see fc.cleanup / OFS_SITL_CLEANUP)",
    })
}

fn send(tx: &UdpSocket, bytes: &[u8], to: SocketAddr) {
    // Windows reports an earlier ICMP "port unreachable" as an error on a later send; a missing
    // reply is detected on receive instead, so send errors are ignored here.
    let _ = tx.send_to(bytes, to);
}

/// A receive error that only means "nothing arrived (yet)".
fn no_reply(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::ConnectionReset)
}

/// Sends `datagram` to `to` and waits on `rx` for one reply, resending the same datagram every
/// `resend_every` until a reply arrives or `timeout` elapses (then `ErrorKind::TimedOut`).
/// Returns the reply length in `buf` and how many times the datagram was resent.
///
/// Lockstep SITL only replies to datagrams it received, so a lost first datagram (state port not yet
/// bound, first packet through the WSL NAT dropped) is never recovered by waiting alone.
pub fn exchange_with_resend(
    rx: &UdpSocket,
    tx: &UdpSocket,
    to: SocketAddr,
    datagram: &[u8],
    buf: &mut [u8],
    timeout: Duration,
    resend_every: Duration,
) -> std::io::Result<(usize, u32)> {
    let deadline = Instant::now() + timeout;
    send(tx, datagram, to);
    let mut next_send = Instant::now() + resend_every;
    let mut resends = 0;
    loop {
        let mut now = Instant::now();
        if now >= deadline {
            return Err(ErrorKind::TimedOut.into());
        }
        if now >= next_send {
            send(tx, datagram, to);
            resends += 1;
            now = Instant::now();
            next_send = now + resend_every;
        }
        let wait = next_send.min(deadline).saturating_duration_since(now).max(Duration::from_millis(1));
        rx.set_read_timeout(Some(wait))?;
        match rx.recv_from(buf) {
            Ok((n, _)) => return Ok((n, resends)),
            Err(e) if no_reply(&e) => {}
            Err(e) => return Err(e),
        }
    }
}

impl Model for SitlBridge {
    fn name(&self) -> &str {
        "fc.sitl"
    }

    fn rate_divisor(&self) -> u32 {
        self.cfg.rate_divisor
    }

    fn step(&mut self, ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        // Exchanges stay paired: drop any stray reply (e.g. SITL answering both copies of a resent first
        // datagram) before sending (M0 §7).
        self.drain().map_err(|e| SimError::Firmware(format!("UDP setup failed: {e}")))?;
        let i = &self.inputs;
        let frame = SensorFrame {
            time_s: ctx.time_s,
            gyro_frd_radps: bus.get(i.gyro),
            accel_frd_mps2: bus.get(i.accel),
            att_ned: bus.get(i.att),
            vel_ned_mps: bus.get(i.vel),
            pos_ned_m: bus.get(i.pos),
            pressure_pa: bus.get(i.pressure),
        };
        // Pilot input reaches Betaflight only as CRSF on its receiver UART (spec §1.5). The UDP RC channels are
        // zero: invalid pulses, so a SITL whose EEPROM still selects the UDP receiver fails safe instead of flying.
        let rc = RcPacket { timestamp_s: ctx.time_s, channels: [0; 16] };
        let datagram = state_datagram_with_serial(&fdm_packet(&frame, &self.cfg.home), &rc, &self.serial_blocks());

        let mut buf = [0u8; 128];
        let mut received = self.exchange(&datagram, &mut buf);
        if matches!(&received, Err(e) if no_reply(e)) && self.rebooted() {
            self.restart()?;
            bus.set(self.restarts, bus.get(self.restarts) + 1.0);
            tracing::info!("Betaflight rebooted; SITL relaunched from its EEPROM");
            received = self.exchange(&datagram, &mut buf);
        }
        match received {
            Ok(n) => {
                let packet = ServoPacket::decode(&buf[..n])
                    .ok_or_else(|| SimError::Firmware(format!("malformed motor packet ({n} bytes)")))?;
                for (sig, v) in self.cmds.iter().zip(motor_commands(&packet)) {
                    bus.set(*sig, v);
                }
                Ok(())
            }
            Err(e) if no_reply(&e) => {
                let waited = if self.answered { self.cfg.reply_timeout } else { self.cfg.first_reply_timeout };
                let what = format!("no motor output within {} ms", waited.as_millis());
                Err(self.firmware_error(&what))
            }
            Err(e) => Err(SimError::Firmware(format!("UDP receive failed: {e}"))),
        }
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --workspace`
Expected: all pass, no warnings.

Then run with `OFS_SITL_LAUNCH` set, 5 times:

```bash
cargo test -p ofs-sim --test sitl_live -- --ignored --test-threads=1
```

Expected: 4 passed every time, and no SITL left afterwards. If a run fails, follow "Known intermittents" at the top of the plan.

- [ ] **Step 4: Commit**

```bash
git add crates/ofs-fc crates/ofs-sim/tests/sitl_live.rs
git commit -m "feat(fc): treat a Betaflight reboot as a firmware restart and relaunch SITL"
```

---

### Task 10: Real-time pacer

**Files:**
- Create: `crates/ofs-sim/src/pacer.rs`, `crates/ofs-sim/tests/pacer.rs`
- Modify: `crates/ofs-sim/src/lib.rs`

**Interfaces:**
- Produces, in `ofs_sim::pacer`:
  - `OverrunPolicy { Warn, Slow }`;
  - `Plan { ticks: u64, sleep_s: f64, overrun: bool }`;
  - `Pacer::new(policy, base_hz)`, `restart(now_s, sim_s)`, `plan(now_s, sim_s) -> Plan`, `overruns() -> u64`;
  - constants `MAX_LAG_S = 0.1`, `CHUNK_S = 0.05`, `MIN_BATCH_S = 0.001`, `YIELD_S = 0.001`, `MAX_SLEEP_S = 0.005`.
- Pure arithmetic on seconds, so it is tested with synthetic clocks. Policy semantics are in the module doc (Decision 5).

- [ ] **Step 1: Write the failing tests** `crates/ofs-sim/tests/pacer.rs`:

```rust
use ofs_sim::pacer::{OverrunPolicy, Pacer, Plan, MAX_SLEEP_S, YIELD_S};

const HZ: u32 = 8000;

fn pacer(policy: OverrunPolicy) -> Pacer {
    let mut p = Pacer::new(policy, HZ);
    p.restart(100.0, 0.0); // wall clock at 100 s, simulation at 0 s
    p
}

#[test]
fn on_schedule_it_runs_the_elapsed_ticks() {
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0101, 0.0), Plan { ticks: 80, sleep_s: YIELD_S, overrun: false });
}

#[test]
fn ahead_of_the_wall_clock_it_sleeps() {
    let mut p = pacer(OverrunPolicy::Warn);
    let plan = p.plan(100.010, 0.0105);
    assert_eq!(plan.ticks, 0);
    assert!(plan.sleep_s >= YIELD_S && plan.sleep_s <= MAX_SLEEP_S, "{plan:?}");
}

#[test]
fn warn_catches_up_in_chunks_within_the_allowed_lag() {
    let mut p = pacer(OverrunPolicy::Warn);
    assert_eq!(p.plan(100.0801, 0.0), Plan { ticks: 400, sleep_s: YIELD_S, overrun: false });
    assert_eq!(p.overruns(), 0);
}

#[test]
fn warn_drops_a_backlog_beyond_the_allowed_lag_and_counts_an_overrun() {
    let mut p = pacer(OverrunPolicy::Warn);
    let plan = p.plan(100.5, 0.0);
    assert!(plan.overrun);
    assert_eq!(plan.ticks, 400);
    assert_eq!(p.overruns(), 1);
    // The backlog is now 0.1 s: catching up continues without new overruns.
    let plan = p.plan(100.5, 0.05);
    assert_eq!((plan.ticks, plan.overrun), (400, false));
    assert_eq!(p.overruns(), 1);
}

#[test]
fn slow_never_bursts_and_counts_an_overrun_per_100_ms_of_stretch() {
    let mut p = pacer(OverrunPolicy::Slow);
    let mut sim = 0.0;
    let mut now = 100.0;
    let mut overruns = Vec::new();
    for _ in 0..5 {
        now += 0.08; // the simulation only manages 0.05 s per 0.08 s of wall time
        let plan = p.plan(now, sim);
        assert_eq!(plan.ticks, 400, "at most one chunk");
        sim += plan.ticks as f64 / f64::from(HZ);
        overruns.push(plan.overrun);
    }
    // 0.03 s of stretch per step: the 4th step crosses 0.1 s.
    assert_eq!(overruns, vec![false, false, false, true, false]);
    assert_eq!(p.overruns(), 1);
}

#[test]
fn restart_re_anchors_after_a_pause() {
    let mut p = pacer(OverrunPolicy::Warn);
    p.restart(500.0, 0.5); // paused for minutes
    let plan = p.plan(500.0011, 0.5);
    assert_eq!((plan.ticks, plan.overrun), (8, false));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p ofs-sim --test pacer`
Expected: compile error, no module `pacer`.

- [ ] **Step 3: Implement** `crates/ofs-sim/src/pacer.rs`:

```rust
//! Real-time pacing (spec §5.1): how many base ticks to run now so simulated time follows the wall clock.
//! Pure arithmetic on seconds, so it is tested with synthetic clocks; the runner thread feeds it real time.
//!
//! When the simulation falls behind:
//! - `Warn` catches up in bursts while the backlog stays within [`MAX_LAG_S`]; beyond that the backlog is
//!   dropped (simulated time slips against the wall clock) and one overrun is counted.
//! - `Slow` never bursts more than one chunk: simulated time stretches instead, and one overrun is counted
//!   for every [`MAX_LAG_S`] of accumulated stretch.
//!
//! Either way the simulation itself stays exact (it is stepped tick by tick); only its alignment with the
//! wall clock suffers, and the overrun count says so.

/// Backlog `Warn` catches up on before dropping it; also the stretch that counts as one `Slow` overrun.
pub const MAX_LAG_S: f64 = 0.1;
/// Most simulated time run per pacing step, so other threads get the session lock regularly.
pub const CHUNK_S: f64 = 0.05;
/// Smallest backlog worth a pacing step (one Betaflight exchange at 1 kHz).
pub const MIN_BATCH_S: f64 = 0.001;
/// Sleep after a pacing step that ran ticks: lets other threads take the session lock.
pub const YIELD_S: f64 = 0.001;
/// Longest sleep while ahead of the wall clock, so pause/resume and new work are picked up quickly.
pub const MAX_SLEEP_S: f64 = 0.005;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrunPolicy {
    Warn,
    Slow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plan {
    /// Base ticks to run now.
    pub ticks: u64,
    /// Seconds to sleep after running them.
    pub sleep_s: f64,
    /// True when this step counted an overrun.
    pub overrun: bool,
}

#[derive(Debug, Clone)]
pub struct Pacer {
    policy: OverrunPolicy,
    base_hz: u32,
    anchor_wall_s: f64,
    anchor_sim_s: f64,
    stretched_s: f64,
    overruns: u64,
}

impl Pacer {
    pub fn new(policy: OverrunPolicy, base_hz: u32) -> Self {
        assert!(base_hz > 0, "base_hz must be > 0");
        Self { policy, base_hz, anchor_wall_s: 0.0, anchor_sim_s: 0.0, stretched_s: 0.0, overruns: 0 }
    }

    /// Aligns simulated time `sim_s` with wall time `now_s`; call when (re)starting real-time running.
    pub fn restart(&mut self, now_s: f64, sim_s: f64) {
        self.anchor_wall_s = now_s;
        self.anchor_sim_s = sim_s;
        self.stretched_s = 0.0;
    }

    pub fn overruns(&self) -> u64 {
        self.overruns
    }

    fn chunk_ticks(&self) -> u64 {
        (CHUNK_S * f64::from(self.base_hz)).round() as u64
    }

    pub fn plan(&mut self, now_s: f64, sim_s: f64) -> Plan {
        let mut lag = self.anchor_sim_s + (now_s - self.anchor_wall_s) - sim_s;
        let allowed = match self.policy {
            OverrunPolicy::Warn => MAX_LAG_S,
            OverrunPolicy::Slow => CHUNK_S,
        };
        let mut overrun = false;
        if lag > allowed {
            let excess = lag - allowed;
            self.anchor_wall_s = now_s;
            self.anchor_sim_s = sim_s + allowed;
            lag = allowed;
            overrun = match self.policy {
                OverrunPolicy::Warn => true,
                OverrunPolicy::Slow => {
                    self.stretched_s += excess;
                    if self.stretched_s >= MAX_LAG_S {
                        self.stretched_s -= MAX_LAG_S;
                        true
                    } else {
                        false
                    }
                }
            };
            if overrun {
                self.overruns += 1;
            }
        }
        if lag < MIN_BATCH_S {
            return Plan { ticks: 0, sleep_s: (MIN_BATCH_S - lag).clamp(YIELD_S, MAX_SLEEP_S), overrun };
        }
        let ticks = ((lag * f64::from(self.base_hz)).floor() as u64).min(self.chunk_ticks());
        Plan { ticks, sleep_s: YIELD_S, overrun }
    }
}
```

In `crates/ofs-sim/src/lib.rs`, add `pub mod pacer;` above `pub mod server;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --test pacer`
Expected: 6 pass, no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-sim
git commit -m "feat(sim): real-time pacer with warn and slow overrun policies"
```

---

### Task 11: Protocol 2 — sessions, the real-time runner, cancellable Run, faults, graceful shutdown

**Files:**
- Replace:
  - `proto/ofs/v1/sim.proto`;
  - `crates/ofs-sim/Cargo.toml`;
  - `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/src/main.rs`, `crates/ofs-sim/src/lib.rs`;
  - `crates/ofs-sim/tests/grpc.rs`
- Create: `crates/ofs-sim/src/session.rs`, `crates/ofs-sim/src/runner.rs`
- Modify: `Cargo.toml` (tokio features)

**Interfaces:**
- **Consumes:**
  - `Vehicle::{step_ticks, time_s, base_hz, configurator_address, set_fault, clear_faults, state}` (Task 8);
  - `Pacer` (Task 10).
- **Produces:**
  - protocol version 2 (the full `.proto` below);
  - `ofs_sim::session::{RunMode, Session, Slot, Shared, event}`;
  - `ofs_sim::runner::{spawn, pace_once}`;
  - `SimService::{new, subscribe() -> broadcast::Receiver<pb::Event>, shutdown()}`;
  - `pub(crate)` helpers `error`, `sim_error`, `not_loaded`, `loaded`, `parse_sticks`, used by `streams` in Task 12.
  - `StreamState`, `Pilot` and `Watch` return `Unimplemented` until Task 12.
- **Behaviour:**
  - Real-time sessions load paused. `Start`/`Pause` need `mode = realtime` (else `invalid_state`).
  - `Run` is refused while a real-time session runs (`invalid_state`) and single-steps a paused one.
  - `Run` steps in 50 ms chunks and stops when its client goes away.
  - Every Run chunk and runner step publishes `link_up`/`link_down`/`firmware_restarted` events (plus `sim_error` and `overrun`) on a broadcast channel.
  - `ofs-sim` shuts down on Ctrl-C (and SIGTERM on unix): it waits up to 3 s for open streams, then unloads (stopping SITL).

- [ ] **Step 1: Write the failing tests.** In the root `Cargo.toml`, change the tokio line to:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "net", "signal", "sync", "time"] }
```

Replace `crates/ofs-sim/tests/grpc.rs` with:

```rust
use std::time::Duration;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, RadioLinkLoss, RunRequest, Sticks};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

const QUAD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml");

async fn start_with_service() -> (SimClient<Channel>, SimService) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let data_dir = std::env::temp_dir().join("ofs-grpc-test-data");
    let service = SimService::new(data_dir);
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(SimServer::new(service.clone()))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    (SimClient::connect(format!("http://{addr}")).await.unwrap(), service)
}

async fn start() -> SimClient<Channel> {
    start_with_service().await.0
}

fn kind(s: &Status) -> String {
    s.metadata().get("ofs-error-kind").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

fn open_loop(mode: Mode) -> LoadRequest {
    LoadRequest { quad_path: QUAD.into(), seed: 1, mode: mode as i32, open_loop_fc: true, ..Default::default() }
}

async fn load_open_loop(c: &mut SimClient<Channel>) {
    c.load(open_loop(Mode::Lockstep)).await.unwrap();
}

async fn time_s(c: &mut SimClient<Channel>) -> f64 {
    c.get_state(Empty {}).await.unwrap().into_inner().time_s
}

#[tokio::test]
async fn handshake_checks_protocol_version() {
    let mut c = start().await;
    let ok = c.handshake(HandshakeRequest { protocol_version: PROTOCOL_VERSION }).await.unwrap().into_inner();
    assert_eq!(ok.protocol_version, PROTOCOL_VERSION);
    let err = c.handshake(HandshakeRequest { protocol_version: 999 }).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(kind(&err), "protocol");
}

#[tokio::test]
async fn run_before_load_is_rejected() {
    let mut c = start().await;
    let err = c.run(RunRequest { seconds: 0.1 }).await.unwrap_err();
    assert_eq!(err.code(), Code::FailedPrecondition);
    assert_eq!(kind(&err), "not_loaded");
}

#[tokio::test]
async fn bad_quad_path_is_a_config_error() {
    let mut c = start().await;
    let req = LoadRequest { quad_path: "does/not/exist.toml".into(), open_loop_fc: true, ..Default::default() };
    let err = c.load(req).await.unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);
    assert_eq!(kind(&err), "config");
    assert!(err.message().contains("does/not/exist.toml"), "{}", err.message());
}

#[tokio::test]
async fn open_loop_session_runs_and_reports_state() {
    let mut c = start().await;
    let reply = c.load(open_loop(Mode::Lockstep)).await.unwrap().into_inner();
    assert_eq!(reply.configurator_address, "", "no Betaflight, no Configurator");
    let s = c.run(RunRequest { seconds: 1.0 }).await.unwrap().into_inner();
    assert!((s.time_s - 1.0).abs() < 1e-9);
    assert!((s.position_ned_m.unwrap().z + 0.0295).abs() < 1e-3);
    assert_eq!(s.motor_rpm.len(), 4);
    let radio = s.radio.unwrap();
    assert!(radio.tx_enabled && radio.link_up, "{radio:?}");
    assert_eq!(radio.lq_pct, 100.0);
    assert!(!s.running);
    let again = c.get_state(Empty {}).await.unwrap().into_inner();
    assert_eq!(again.time_s, s.time_s);
}

#[tokio::test]
async fn invalid_run_durations_do_not_poison_the_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    for seconds in [-1.0, f64::NAN] {
        let err = c.run(RunRequest { seconds }).await.unwrap_err();
        assert_eq!(kind(&err), "invalid_argument");
    }
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
}

#[tokio::test]
async fn non_finite_sticks_are_rejected() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c.set_sticks(Sticks { roll: f64::NAN, ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    let err = c.set_sticks(Sticks { aux: vec![0.0; 5], ..Default::default() }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    c.set_sticks(Sticks { throttle: 0.5, aux: vec![1.0], ..Default::default() }).await.unwrap();
}

#[tokio::test]
async fn realtime_sessions_load_paused_and_follow_the_wall_clock() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(time_s(&mut c).await, 0.0, "loads paused");
    c.start(Empty {}).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let s = c.get_state(Empty {}).await.unwrap().into_inner();
    assert!(s.running);
    assert!((0.6..=1.4).contains(&s.time_s), "{} s simulated in 1 s of wall time", s.time_s);
    c.pause(Empty {}).await.unwrap();
    let paused_at = time_s(&mut c).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(time_s(&mut c).await, paused_at);
}

#[tokio::test]
async fn run_needs_a_paused_or_lockstep_session_and_start_needs_realtime() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let err = c.start(Empty {}).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let err = c.run(RunRequest { seconds: 0.1 }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_state");
    c.pause(Empty {}).await.unwrap();
    let before = time_s(&mut c).await;
    let s = c.run(RunRequest { seconds: 0.1 }).await.unwrap().into_inner(); // single-step while paused
    assert!((s.time_s - before - 0.1).abs() < 1e-9);
}

#[tokio::test]
async fn an_abandoned_run_stops_and_releases_the_session() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let mut req = Request::new(RunRequest { seconds: 100_000.0 });
    req.set_timeout(Duration::from_millis(300));
    let err = c.run(req).await.unwrap_err();
    // The client-side deadline cancels the call (tonic reports Cancelled or DeadlineExceeded).
    assert!(matches!(err.code(), Code::DeadlineExceeded | Code::Cancelled), "{err:?}");
    let t = tokio::time::timeout(Duration::from_secs(5), time_s(&mut c)).await.expect("the session stayed locked");
    assert!(t < 100_000.0);
}

#[tokio::test]
async fn faults_drop_the_radio_link_and_events_report_it() {
    let (mut c, service) = start_with_service().await;
    let mut events = service.subscribe();
    load_open_loop(&mut c).await;
    c.run(RunRequest { seconds: 0.5 }).await.unwrap();
    let fault = Fault { kind: Some(fault::Kind::RadioLinkLoss(RadioLinkLoss {})) };
    c.inject_fault(fault).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    assert!(!s.radio.unwrap().link_up);
    c.clear_faults(Empty {}).await.unwrap();
    let s = c.run(RunRequest { seconds: 0.5 }).await.unwrap().into_inner();
    assert!(s.radio.unwrap().link_up);
    let mut kinds = Vec::new();
    while let Ok(e) = events.try_recv() {
        kinds.push(e.kind());
    }
    assert_eq!(kinds, vec![EventKind::LinkUp, EventKind::LinkDown, EventKind::LinkUp]);
    let err = c.inject_fault(Fault { kind: None }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
}
```

Run: `cargo test -p ofs-sim --test grpc`
Expected: compile errors (`Mode::Realtime`, `fault`, `start`, `subscribe`, …).

- [ ] **Step 2: Protocol 2.** Replace `proto/ofs/v1/sim.proto` with:

```proto
syntax = "proto3";

package ofs.v1;

// Simulator control, protocol version 2: lockstep and real-time sessions, state and event streams, the
// pilot link and fault injection. One session (loaded quad) per server.
service Sim {
  rpc Handshake(HandshakeRequest) returns (HandshakeReply);
  rpc Load(LoadRequest) returns (LoadReply);
  rpc SetSticks(Sticks) returns (Empty);
  // Steps a lockstep session, or a paused real-time session (single-stepping).
  rpc Run(RunRequest) returns (State);
  // Real-time sessions load paused; Start paces them to the wall clock, Pause stops them.
  rpc Start(Empty) returns (Empty);
  rpc Pause(Empty) returns (Empty);
  rpc GetState(Empty) returns (State);
  rpc Unload(Empty) returns (Empty);
  // Vehicle state at a client rate until the client cancels or the session is unloaded.
  rpc StreamState(StreamRequest) returns (stream State);
  // The pilot's transmitter: sticks in, state out. While the stream is open the transmitter is on; when it
  // closes (or the client vanishes) the transmitter goes off and Betaflight fails safe. One pilot at a time.
  rpc Pilot(stream PilotInput) returns (stream State);
  // Session events. A session loaded with keep_alive = false ends when its last watcher disconnects.
  rpc Watch(Empty) returns (stream Event);
  rpc InjectFault(Fault) returns (Empty);
  rpc ClearFaults(Empty) returns (Empty);
}

message Empty {}

message HandshakeRequest { uint32 protocol_version = 1; }
message HandshakeReply {
  uint32 protocol_version = 1;
  string server_version = 2;
}

enum Mode {
  MODE_UNSPECIFIED = 0;  // treated as lockstep
  MODE_LOCKSTEP = 1;
  MODE_REALTIME = 2;
}

// What a real-time session does when it falls behind the wall clock (spec §5.1).
enum OverrunPolicy {
  OVERRUN_POLICY_UNSPECIFIED = 0;  // treated as warn
  OVERRUN_POLICY_WARN = 1;         // catch up in bursts; drop a backlog over 100 ms and count an overrun
  OVERRUN_POLICY_SLOW = 2;         // never burst; simulated time stretches, one overrun per 100 ms of stretch
}

message LoadRequest {
  string quad_path = 1;
  uint64 seed = 2;
  Mode mode = 3;
  bool open_loop_fc = 4;  // testing aid: replace the configured FC with throttle-only open loop
  OverrunPolicy overrun_policy = 5;
  bool keep_alive = 6;  // keep the session when its last watcher disconnects
}
message LoadReply {
  string quad_name = 1;
  uint32 base_hz = 2;
  string configurator_address = 3;  // tcp://127.0.0.1:5761 when the FC is Betaflight SITL, else empty
}

// roll, pitch, yaw, aux in [-1, 1]; throttle in [0, 1]. Up to 4 aux channels; missing ones are -1.
message Sticks {
  double roll = 1;
  double pitch = 2;
  double yaw = 3;
  double throttle = 4;
  repeated double aux = 5;
}

message RunRequest { double seconds = 1; }

// rate_hz: 1..240; 0 means 60.
message StreamRequest { uint32 rate_hz = 1; }

message PilotInput {
  Sticks sticks = 1;
  uint32 state_rate_hz = 2;  // read from the first message only; 0 means 60
}

message Vec3 {
  double x = 1;
  double y = 2;
  double z = 3;
}
message Quat {
  double w = 1;
  double x = 2;
  double y = 3;
  double z = 4;
}

message RadioLink {
  bool tx_enabled = 1;  // a pilot or script is connected
  bool link_up = 2;
  double lq_pct = 3;
  double rssi_dbm = 4;
}

message State {
  double time_s = 1;
  Vec3 position_ned_m = 2;
  Vec3 velocity_ned_mps = 3;
  Quat attitude = 4;  // FRD body -> NED world
  Vec3 rate_frd_radps = 5;
  double battery_voltage_v = 6;
  double battery_current_a = 7;
  repeated double motor_rpm = 8;
  repeated double motor_cmd = 9;
  RadioLink radio = 10;
  bool running = 11;        // a real-time session paced to the wall clock right now
  uint64 overruns = 12;     // real-time overruns so far
  uint32 fc_restarts = 13;  // Betaflight reboots so far
}

enum EventKind {
  EVENT_KIND_UNSPECIFIED = 0;
  EVENT_KIND_OVERRUN = 1;
  EVENT_KIND_FIRMWARE_RESTARTED = 2;
  EVENT_KIND_SIM_ERROR = 3;
  EVENT_KIND_LINK_DOWN = 4;
  EVENT_KIND_LINK_UP = 5;
  EVENT_KIND_PILOT_CONNECTED = 6;
  EVENT_KIND_PILOT_DISCONNECTED = 7;
  EVENT_KIND_SESSION_ENDED = 8;
}

message Event {
  double time_s = 1;  // simulated time
  EventKind kind = 2;
  string message = 3;
}

message RadioLinkLoss {}

message Fault {
  oneof kind {
    RadioLinkLoss radio_link_loss = 1;  // every uplink packet is lost while active
  }
}
```

Replace `crates/ofs-sim/Cargo.toml`. `tokio-stream` moves to normal dependencies, and `tracing` is added:

```toml
[package]
name = "ofs-sim"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
ofs-core.workspace = true
ofs-physics.workspace = true
ofs-electrical.workspace = true
ofs-sensors.workspace = true
ofs-config.workspace = true
ofs-fc.workspace = true
ofs-radio.workspace = true
glam.workspace = true
tonic.workspace = true
prost.workspace = true
tokio.workspace = true
tokio-stream.workspace = true
clap.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true

[build-dependencies]
tonic-build.workspace = true
protoc-bin-vendored.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 3: Sessions and the runner.** Create `crates/ofs-sim/src/session.rs`:

```rust
//! The loaded session and the state every part of the server shares: gRPC handlers, the real-time runner
//! and the streams.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use ofs_core::SimError;
use tokio::sync::broadcast;

use crate::pacer::{OverrunPolicy, Pacer};
use crate::pb;
use crate::vehicle::{Vehicle, VehicleState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Lockstep,
    Realtime,
}

pub struct Session {
    pub vehicle: Vehicle,
    pub mode: RunMode,
    /// Real-time sessions only: paced to the wall clock right now.
    pub running: bool,
    /// Set after a firmware or numerical failure; the session stays paused until the next Load.
    pub failure: Option<SimError>,
    pub pacer: Pacer,
    /// Keep the session when its last watcher disconnects.
    pub keep_alive: bool,
    /// A watcher has seen this session: only watched sessions end with their watchers.
    pub watched: bool,
    last_link_up: bool,
    last_restarts: u32,
}

pub type Slot = Option<Session>;

pub fn event(time_s: f64, kind: pb::EventKind, message: impl Into<String>) -> pb::Event {
    pb::Event { time_s, kind: kind as i32, message: message.into() }
}

fn vec3(v: glam::DVec3) -> Option<pb::Vec3> {
    Some(pb::Vec3 { x: v.x, y: v.y, z: v.z })
}

impl Session {
    pub fn new(vehicle: Vehicle, mode: RunMode, policy: OverrunPolicy, keep_alive: bool) -> Self {
        let pacer = Pacer::new(policy, vehicle.base_hz());
        Self {
            vehicle,
            mode,
            running: false,
            failure: None,
            pacer,
            keep_alive,
            watched: false,
            last_link_up: false,
            last_restarts: 0,
        }
    }

    /// Steps `ticks` base ticks. A firmware or numerical failure poisons the session and pauses it.
    pub fn step(&mut self, ticks: u64) -> Result<(), SimError> {
        if let Some(f) = &self.failure {
            return Err(f.clone());
        }
        if let Err(e) = self.vehicle.step_ticks(ticks) {
            if matches!(e, SimError::Firmware(_) | SimError::NonFinite(_)) {
                self.failure = Some(e.clone());
                self.running = false;
            }
            return Err(e);
        }
        Ok(())
    }

    /// Events implied by the vehicle since the last call: radio link up or down, Betaflight restarts.
    pub fn changes(&mut self) -> Vec<pb::Event> {
        let s = self.vehicle.state();
        let mut out = Vec::new();
        if s.radio.link_up != self.last_link_up {
            self.last_link_up = s.radio.link_up;
            let (kind, message) =
                if s.radio.link_up { (pb::EventKind::LinkUp, "radio link up") } else { (pb::EventKind::LinkDown, "radio link lost") };
            out.push(event(s.time_s, kind, message));
        }
        if s.fc_restarts != self.last_restarts {
            self.last_restarts = s.fc_restarts;
            out.push(event(s.time_s, pb::EventKind::FirmwareRestarted, "Betaflight rebooted; SITL relaunched"));
        }
        out
    }

    pub fn state_msg(&self) -> pb::State {
        let s: VehicleState = self.vehicle.state();
        pb::State {
            time_s: s.time_s,
            position_ned_m: vec3(s.pos_ned_m),
            velocity_ned_mps: vec3(s.vel_ned_mps),
            attitude: Some(pb::Quat { w: s.att.w, x: s.att.x, y: s.att.y, z: s.att.z }),
            rate_frd_radps: vec3(s.rate_frd_radps),
            battery_voltage_v: s.battery_voltage_v,
            battery_current_a: s.battery_current_a,
            motor_rpm: s.motor_rpm,
            motor_cmd: s.motor_cmd,
            radio: Some(pb::RadioLink {
                tx_enabled: s.radio.tx_enabled,
                link_up: s.radio.link_up,
                lq_pct: s.radio.lq_pct,
                rssi_dbm: s.radio.rssi_dbm,
            }),
            running: self.running,
            overruns: self.pacer.overruns(),
            fc_restarts: s.fc_restarts,
        }
    }
}

/// State shared by the gRPC handlers, the real-time runner thread and the streams.
pub struct Shared {
    slot: Mutex<Slot>,
    epoch: Instant,
    stopping: AtomicBool,
    pub events: broadcast::Sender<pb::Event>,
    /// Open Watch streams.
    pub watchers: AtomicUsize,
    /// A Pilot stream is connected.
    pub pilot: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            slot: Mutex::new(None),
            epoch: Instant::now(),
            stopping: AtomicBool::new(false),
            events,
            watchers: AtomicUsize::new(0),
            pilot: AtomicBool::new(false),
        })
    }

    /// The session slot. A panic while it was held does not lock the server out.
    pub fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Seconds since the server started (the real-time pacer's wall clock).
    pub fn wall_s(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    pub fn publish(&self, events: impl IntoIterator<Item = pb::Event>) {
        for e in events {
            let _ = self.events.send(e); // no subscribers is fine
        }
    }

    pub fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    pub fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }
}
```

Create `crates/ofs-sim/src/runner.rs`:

```rust
//! Paces real-time sessions to the wall clock on a background thread.
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::pacer::MAX_LAG_S;
use crate::pb;
use crate::session::{event, RunMode, Shared};

/// Poll interval while no real-time session is running.
const IDLE: Duration = Duration::from_millis(5);

pub fn spawn(shared: Arc<Shared>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("ofs-realtime".into())
        .spawn(move || {
            while !shared.stopping() {
                let sleep = pace_once(&shared);
                std::thread::sleep(sleep);
            }
        })
        .expect("failed to spawn the real-time runner thread")
}

/// One pacing step of the loaded session, if it is a running real-time session. Returns how long to sleep.
pub fn pace_once(shared: &Shared) -> Duration {
    let mut slot = shared.lock();
    let Some(s) = slot.as_mut() else { return IDLE };
    if s.mode != RunMode::Realtime || !s.running {
        return IDLE;
    }
    let plan = s.pacer.plan(shared.wall_s(), s.vehicle.time_s());
    let mut events = Vec::new();
    if plan.overrun {
        let message = format!("fell more than {} ms behind the wall clock", (MAX_LAG_S * 1000.0) as u32);
        events.push(event(s.vehicle.time_s(), pb::EventKind::Overrun, message));
    }
    if plan.ticks > 0 {
        if let Err(e) = s.step(plan.ticks) {
            tracing::error!("real-time session stopped: {e}");
            events.push(event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string()));
        }
        events.extend(s.changes());
    }
    drop(slot);
    shared.publish(events);
    Duration::from_secs_f64(plan.sleep_s)
}
```

- [ ] **Step 4: The service.** Replace `crates/ofs-sim/src/server.rs` with:

```rust
//! gRPC service: one session at a time. Unary calls lock the session on blocking threads, the runner
//! thread paces real-time sessions, and the streaming calls live in `streams`.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ofs_config::FcKind;
use ofs_core::{names::RC_AUX_COUNT, SimError};
use tokio::sync::broadcast;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::MetadataMap;
use tonic::{Code, Request, Response, Status, Streaming};

use crate::pacer::OverrunPolicy;
use crate::pb::{self, sim_server::Sim};
use crate::runner;
use crate::session::{event, RunMode, Session, Shared, Slot};
use crate::vehicle::{self, BuildOptions, Fault, Sticks};

pub const PROTOCOL_VERSION: u32 = 2;

#[derive(Clone)]
pub struct SimService {
    shared: Arc<Shared>,
    data_dir: PathBuf,
}

/// A status carrying the machine-readable `ofs-error-kind` metadata that clients map to typed errors.
pub fn error(kind: &'static str, code: Code, message: impl Into<String>) -> Status {
    let mut md = MetadataMap::new();
    md.insert("ofs-error-kind", kind.parse().expect("kind is ASCII"));
    Status::with_metadata(code, message.into(), md)
}

pub(crate) fn sim_error(e: &SimError) -> Status {
    match e {
        SimError::Firmware(m) => error("firmware", Code::Aborted, m.clone()),
        SimError::NonFinite(_) => error("numerical", Code::Aborted, e.to_string()),
        SimError::InvalidArgument(m) => error("invalid_argument", Code::InvalidArgument, m.clone()),
        SimError::Other(m) => error("internal", Code::Internal, m.clone()),
    }
}

pub(crate) fn not_loaded() -> Status {
    error("not_loaded", Code::FailedPrecondition, "no quad loaded; call Load first")
}

pub(crate) fn loaded(slot: &mut Slot) -> Result<&mut Session, Status> {
    slot.as_mut().ok_or_else(not_loaded)
}

fn invalid_state(message: &str) -> Status {
    error("invalid_state", Code::FailedPrecondition, message)
}

/// Validates sticks from a request (finite values, at most four aux channels; missing aux read -1).
pub(crate) fn parse_sticks(s: pb::Sticks) -> Result<Sticks, Status> {
    if s.aux.len() > RC_AUX_COUNT {
        let message = format!("at most {RC_AUX_COUNT} aux channels (got {})", s.aux.len());
        return Err(error("invalid_argument", Code::InvalidArgument, message));
    }
    let mut aux = [-1.0; RC_AUX_COUNT];
    aux[..s.aux.len()].copy_from_slice(&s.aux);
    let sticks = Sticks { roll: s.roll, pitch: s.pitch, yaw: s.yaw, throttle: s.throttle, aux };
    let all_finite = [sticks.roll, sticks.pitch, sticks.yaw, sticks.throttle].iter().chain(aux.iter()).all(|v| v.is_finite());
    if !all_finite {
        return Err(error("invalid_argument", Code::InvalidArgument, "stick values must be finite"));
    }
    Ok(sticks)
}

/// Sets its flag when dropped: a Run whose client went away (tonic drops its future) stops at the next chunk.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl SimService {
    pub fn new(data_dir: PathBuf) -> Self {
        let shared = Shared::new();
        runner::spawn(shared.clone());
        Self { shared, data_dir }
    }

    /// Every event published from now on (the Watch RPC reads the same channel).
    pub fn subscribe(&self) -> broadcast::Receiver<pb::Event> {
        self.shared.events.subscribe()
    }

    /// Stops the real-time runner and unloads the session, stopping Betaflight SITL. For server shutdown.
    pub fn shutdown(&self) {
        self.shared.stop();
        *self.shared.lock() = None;
    }

    async fn blocking<T, F>(&self, f: F) -> Result<Response<T>, Status>
    where
        T: Send + 'static,
        F: FnOnce(&Shared, &mut Slot) -> Result<T, Status> + Send + 'static,
    {
        let shared = self.shared.clone();
        tokio::task::spawn_blocking(move || {
            let mut slot = shared.lock();
            f(&shared, &mut slot)
        })
        .await
        .map_err(|e| error("internal", Code::Internal, e.to_string()))?
        .map(Response::new)
    }
}

#[tonic::async_trait]
impl Sim for SimService {
    async fn handshake(&self, req: Request<pb::HandshakeRequest>) -> Result<Response<pb::HandshakeReply>, Status> {
        let v = req.into_inner().protocol_version;
        if v != PROTOCOL_VERSION {
            return Err(error("protocol", Code::FailedPrecondition, format!("client speaks protocol {v}, server speaks {PROTOCOL_VERSION}")));
        }
        Ok(Response::new(pb::HandshakeReply { protocol_version: PROTOCOL_VERSION, server_version: env!("CARGO_PKG_VERSION").into() }))
    }

    async fn load(&self, req: Request<pb::LoadRequest>) -> Result<Response<pb::LoadReply>, Status> {
        let req = req.into_inner();
        let mode = match pb::Mode::try_from(req.mode) {
            Ok(pb::Mode::Unspecified | pb::Mode::Lockstep) => RunMode::Lockstep,
            Ok(pb::Mode::Realtime) => RunMode::Realtime,
            Err(_) => return Err(error("invalid_argument", Code::InvalidArgument, format!("unknown mode {}", req.mode))),
        };
        let policy = match pb::OverrunPolicy::try_from(req.overrun_policy) {
            Ok(pb::OverrunPolicy::Unspecified | pb::OverrunPolicy::Warn) => OverrunPolicy::Warn,
            Ok(pb::OverrunPolicy::Slow) => OverrunPolicy::Slow,
            Err(_) => {
                let message = format!("unknown overrun policy {}", req.overrun_policy);
                return Err(error("invalid_argument", Code::InvalidArgument, message));
            }
        };
        let cfg = ofs_config::load(Path::new(&req.quad_path)).map_err(|e| error("config", Code::InvalidArgument, e.to_string()))?;
        let opts = BuildOptions { seed: req.seed, data_dir: self.data_dir.clone(), fc_override: req.open_loop_fc.then_some(FcKind::OpenLoop) };
        let keep_alive = req.keep_alive;
        self.blocking(move |shared, slot| {
            *slot = None; // stop the previous vehicle (and its SITL) before the new one binds the ports
            let vehicle = vehicle::build(&cfg, &opts).map_err(|e| sim_error(&e))?;
            let configurator_address = vehicle.configurator_address().unwrap_or_default();
            let mut session = Session::new(vehicle, mode, policy, keep_alive);
            session.watched = shared.watchers.load(Ordering::Acquire) > 0;
            *slot = Some(session);
            Ok(pb::LoadReply { quad_name: cfg.name.clone(), base_hz: cfg.sim.base_hz, configurator_address })
        })
        .await
    }

    async fn set_sticks(&self, req: Request<pb::Sticks>) -> Result<Response<pb::Empty>, Status> {
        let sticks = parse_sticks(req.into_inner())?;
        self.blocking(move |_, slot| {
            loaded(slot)?.vehicle.set_sticks(&sticks);
            Ok(pb::Empty {})
        })
        .await
    }

    async fn run(&self, req: Request<pb::RunRequest>) -> Result<Response<pb::State>, Status> {
        let seconds = req.into_inner().seconds;
        if !seconds.is_finite() || seconds < 0.0 {
            let message = format!("seconds must be finite and >= 0 (got {seconds})");
            return Err(error("invalid_argument", Code::InvalidArgument, message));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let _cancel_on_drop = CancelOnDrop(cancelled.clone());
        self.blocking(move |shared, slot| {
            let s = loaded(slot)?;
            if s.mode == RunMode::Realtime && s.running {
                return Err(invalid_state("pause the real-time session before calling Run"));
            }
            let hz = u64::from(s.vehicle.base_hz());
            let total = (seconds * hz as f64).round() as u64;
            let chunk = (hz / 20).max(1); // 50 ms of simulated time between cancellation checks
            let healthy = s.failure.is_none();
            let mut done = 0;
            while done < total {
                if cancelled.load(Ordering::Acquire) {
                    return Err(Status::cancelled("the client went away"));
                }
                let n = chunk.min(total - done);
                let result = s.step(n);
                shared.publish(s.changes());
                if let Err(e) = result {
                    if healthy && s.failure.is_some() {
                        shared.publish([event(s.vehicle.time_s(), pb::EventKind::SimError, e.to_string())]);
                    }
                    return Err(sim_error(&e));
                }
                done += n;
            }
            Ok(s.state_msg())
        })
        .await
    }

    async fn start(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|shared, slot| {
            let s = loaded(slot)?;
            if s.mode != RunMode::Realtime {
                return Err(invalid_state("Start and Pause need a session loaded with mode = realtime"));
            }
            if let Some(f) = &s.failure {
                return Err(sim_error(f));
            }
            if !s.running {
                s.pacer.restart(shared.wall_s(), s.vehicle.time_s());
                s.running = true;
            }
            Ok(pb::Empty {})
        })
        .await
    }

    async fn pause(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
            let s = loaded(slot)?;
            if s.mode != RunMode::Realtime {
                return Err(invalid_state("Start and Pause need a session loaded with mode = realtime"));
            }
            s.running = false;
            Ok(pb::Empty {})
        })
        .await
    }

    async fn get_state(&self, _req: Request<pb::Empty>) -> Result<Response<pb::State>, Status> {
        self.blocking(|_, slot| Ok(loaded(slot)?.state_msg())).await
    }

    async fn unload(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
            *slot = None;
            Ok(pb::Empty {})
        })
        .await
    }

    type StreamStateStream = ReceiverStream<Result<pb::State, Status>>;

    async fn stream_state(&self, req: Request<pb::StreamRequest>) -> Result<Response<Self::StreamStateStream>, Status> {
        let _ = req;
        Err(error("internal", Code::Unimplemented, "StreamState is not implemented yet"))
    }

    type PilotStream = ReceiverStream<Result<pb::State, Status>>;

    async fn pilot(&self, req: Request<Streaming<pb::PilotInput>>) -> Result<Response<Self::PilotStream>, Status> {
        let _ = req;
        Err(error("internal", Code::Unimplemented, "Pilot is not implemented yet"))
    }

    type WatchStream = ReceiverStream<Result<pb::Event, Status>>;

    async fn watch(&self, _req: Request<pb::Empty>) -> Result<Response<Self::WatchStream>, Status> {
        Err(error("internal", Code::Unimplemented, "Watch is not implemented yet"))
    }

    async fn inject_fault(&self, req: Request<pb::Fault>) -> Result<Response<pb::Empty>, Status> {
        let fault = match req.into_inner().kind {
            Some(pb::fault::Kind::RadioLinkLoss(_)) => Fault::RadioLinkLoss,
            None => return Err(error("invalid_argument", Code::InvalidArgument, "the fault has no kind")),
        };
        self.blocking(move |_, slot| {
            loaded(slot)?.vehicle.set_fault(fault, true);
            Ok(pb::Empty {})
        })
        .await
    }

    async fn clear_faults(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        self.blocking(|_, slot| {
            loaded(slot)?.vehicle.clear_faults();
            Ok(pb::Empty {})
        })
        .await
    }
}
```

Replace `crates/ofs-sim/src/lib.rs` with:

```rust
//! Open FPV Sim server library: vehicle assembly, sessions, real-time pacing and the gRPC service.
pub mod pacer;
pub mod runner;
pub mod server;
pub mod session;
pub mod vehicle;

pub mod pb {
    tonic::include_proto!("ofs.v1");
}
```

Replace `crates/ofs-sim/src/main.rs` with:

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
    tokio::select! {
        result = server => result?,
        _ = grace => eprintln!("ofs-sim: streams still open after 3 s; closing anyway"),
    }
    service.shutdown(); // stops the real-time runner and Betaflight SITL
    Ok(())
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --workspace`
Expected: the 10 grpc tests and all others pass, no warnings. `Cargo.lock` gains `signal-hook-registry` (tokio's `signal` feature); commit it.

Run the timing-sensitive grpc tests 3 times to check they are stable:

```bash
cargo test -p ofs-sim --test grpc -- realtime abandoned
```

Expected: pass every time.

Manual shutdown check:
1. Run `cargo run -p ofs-sim`.
2. Press Ctrl-C.
3. It prints `ofs-sim: shutting down` and exits within a second.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock proto crates/ofs-sim
git commit -m "feat(sim): protocol 2 sessions with real-time runner, cancellable Run, faults, graceful shutdown"
```

---

### Task 12: Streams — state feed, the pilot link, session watchers

**Files:**
- Create: `crates/ofs-sim/src/streams.rs`
- Modify: `crates/ofs-sim/src/server.rs`, `crates/ofs-sim/src/lib.rs`, `crates/ofs-sim/tests/grpc.rs`

**Interfaces:**
- **Consumes:** `Shared`, `event`, `error`, `loaded`, `not_loaded`, `parse_sticks` (Task 11).
- **Produces:**
  - `ofs_sim::streams::{state_rate, state_feed, pilot, watch, DEFAULT_STATE_RATE_HZ = 60, MAX_STATE_RATE_HZ = 240}`;
  - `StreamState`, `Pilot` and `Watch` implemented. M2b's Godot client uses `Pilot`, `StreamState` and `Watch`.
- **Behaviour:**
  - A second pilot gets `pilot_busy`.
  - When a pilot's input stream ends, the transmitter goes off and `pilot_disconnected` is published.
  - When the last `Watch` closes, a session that a watcher saw and that has `keep_alive = false` is unloaded, and `session_ended` is published.

- [ ] **Step 1: Write the failing tests.** In `crates/ofs-sim/tests/grpc.rs`, replace the `use` block at the top with:

```rust
use std::time::Duration;

use ofs_sim::pb::sim_client::SimClient;
use ofs_sim::pb::sim_server::SimServer;
use ofs_sim::pb::{
    fault, Empty, EventKind, Fault, HandshakeRequest, LoadRequest, Mode, PilotInput, RadioLinkLoss, RunRequest, Sticks,
    StreamRequest,
};
use ofs_sim::server::{SimService, PROTOCOL_VERSION};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic::{Code, Request, Status};
```

and append:

```rust

#[tokio::test]
async fn state_streams_at_the_requested_rate() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let err = c.stream_state(StreamRequest { rate_hz: 1000 }).await.unwrap_err();
    assert_eq!(kind(&err), "invalid_argument");
    let mut stream = c.stream_state(StreamRequest { rate_hz: 50 }).await.unwrap().into_inner();
    let started = std::time::Instant::now();
    let mut times = Vec::new();
    while times.len() < 10 {
        times.push(stream.next().await.unwrap().unwrap().time_s);
    }
    let elapsed = started.elapsed().as_secs_f64();
    assert!((0.15..=0.6).contains(&elapsed), "10 states at 50 Hz took {elapsed} s");
    assert!(times.windows(2).all(|w| w[1] >= w[0]), "{times:?}");
}

#[tokio::test]
async fn a_vanished_pilot_switches_the_transmitter_off() {
    let mut c = start().await;
    c.load(open_loop(Mode::Realtime)).await.unwrap();
    c.start(Empty {}).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tx.send(PilotInput { sticks: Some(Sticks { throttle: 0.2, ..Default::default() }), state_rate_hz: 50 }).await.unwrap();
    let mut states = c.pilot(ReceiverStream::new(rx)).await.unwrap().into_inner();
    let s = states.next().await.unwrap().unwrap();
    assert!(s.radio.unwrap().tx_enabled);

    let (tx2, rx2) = tokio::sync::mpsc::channel(1);
    tx2.send(PilotInput::default()).await.unwrap();
    let err = c.pilot(ReceiverStream::new(rx2)).await.unwrap_err();
    assert_eq!(kind(&err), "pilot_busy");

    drop(tx); // the pilot's input stream ends: the client is gone
    drop(states);
    let mut radio = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        radio = c.get_state(Empty {}).await.unwrap().into_inner().radio;
        if !radio.unwrap().tx_enabled && !radio.unwrap().link_up {
            break;
        }
    }
    let radio = radio.unwrap();
    assert!(!radio.tx_enabled && !radio.link_up, "{radio:?}");
    assert!(time_s(&mut c).await > 0.0, "the session keeps running for the pilot to reconnect");
}

#[tokio::test]
async fn the_last_watcher_leaving_ends_a_session_unless_keep_alive() {
    let mut c = start().await;
    for keep_alive in [false, true] {
        c.load(LoadRequest { keep_alive, ..open_loop(Mode::Lockstep) }).await.unwrap();
        let watch = c.watch(Empty {}).await.unwrap().into_inner();
        drop(watch);
        let mut loaded = true;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            loaded = c.get_state(Empty {}).await.is_ok();
            if !loaded {
                break;
            }
        }
        assert_eq!(loaded, keep_alive, "keep_alive = {keep_alive}");
    }
}

#[tokio::test]
async fn watchers_receive_events() {
    let mut c = start().await;
    load_open_loop(&mut c).await;
    let mut watch = c.watch(Empty {}).await.unwrap().into_inner();
    c.run(RunRequest { seconds: 0.1 }).await.unwrap();
    let e = tokio::time::timeout(Duration::from_secs(5), watch.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(e.kind(), EventKind::LinkUp);
}
```

Run: `cargo test -p ofs-sim --test grpc`
Expected: the 4 new tests fail with `Unimplemented`.

- [ ] **Step 2: Implement** `crates/ofs-sim/src/streams.rs`:

```rust
//! Streaming RPCs: state at a client rate, the pilot's transmitter link, and session watchers.
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Code, Status, Streaming};

use crate::pb;
use crate::server::{error, loaded, not_loaded, parse_sticks};
use crate::session::{event, Shared};

pub const DEFAULT_STATE_RATE_HZ: u32 = 60;
/// Spec §5.2: client state streams run at up to 240 Hz.
pub const MAX_STATE_RATE_HZ: u32 = 240;

pub fn state_rate(hz: u32) -> Result<u32, Status> {
    match hz {
        0 => Ok(DEFAULT_STATE_RATE_HZ),
        1..=MAX_STATE_RATE_HZ => Ok(hz),
        _ => Err(error("invalid_argument", Code::InvalidArgument, format!("state rate must be 1..={MAX_STATE_RATE_HZ} Hz (got {hz})"))),
    }
}

/// Sends the session state every 1/rate s from a plain thread, until the client goes away, the server stops,
/// or the session is unloaded (then the stream ends with a not_loaded error).
pub fn state_feed(shared: Arc<Shared>, rate_hz: u32) -> ReceiverStream<Result<pb::State, Status>> {
    let (tx, rx) = mpsc::channel(4);
    let period = Duration::from_secs_f64(1.0 / f64::from(rate_hz));
    std::thread::spawn(move || loop {
        std::thread::sleep(period);
        if tx.is_closed() || shared.stopping() {
            break;
        }
        let msg = shared.lock().as_ref().map(|s| s.state_msg()).ok_or_else(not_loaded);
        let last = msg.is_err();
        if tx.blocking_send(msg).is_err() || last {
            break;
        }
    });
    ReceiverStream::new(rx)
}

/// Runs `f` off the async runtime (it locks the session, which can wait for a runner step).
fn off_runtime(f: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => drop(handle.spawn_blocking(f)),
        Err(_) => f(),
    }
}

/// Held while a pilot is connected. Dropping it (the input stream ended or failed) turns the transmitter off,
/// so Betaflight fails safe as with a real radio switched off, and frees the pilot slot.
struct PilotGuard(Arc<Shared>);

impl Drop for PilotGuard {
    fn drop(&mut self) {
        let shared = self.0.clone();
        off_runtime(move || {
            let time_s = shared.lock().as_mut().map(|s| {
                s.vehicle.set_transmitter(false);
                s.vehicle.time_s()
            });
            shared.pilot.store(false, Ordering::Release);
            if let Some(t) = time_s {
                shared.publish([event(t, pb::EventKind::PilotDisconnected, "pilot disconnected: transmitter off")]);
            }
        });
    }
}

pub async fn pilot(
    shared: Arc<Shared>,
    mut inbound: Streaming<pb::PilotInput>,
) -> Result<ReceiverStream<Result<pb::State, Status>>, Status> {
    let first = inbound
        .message()
        .await?
        .ok_or_else(|| error("invalid_argument", Code::InvalidArgument, "send a PilotInput to start piloting"))?;
    let rate_hz = state_rate(first.state_rate_hz)?;
    let first_sticks = parse_sticks(first.sticks.unwrap_or_default())?;
    if shared.pilot.swap(true, Ordering::AcqRel) {
        return Err(error("pilot_busy", Code::AlreadyExists, "a pilot is already connected"));
    }
    let guard = PilotGuard(shared.clone());
    let s = shared.clone();
    tokio::task::spawn_blocking(move || -> Result<(), Status> {
        let mut slot = s.lock();
        let session = loaded(&mut slot)?;
        session.vehicle.set_sticks(&first_sticks);
        session.vehicle.set_transmitter(true);
        let t = session.vehicle.time_s();
        drop(slot);
        s.publish([event(t, pb::EventKind::PilotConnected, "pilot connected: transmitter on")]);
        Ok(())
    })
    .await
    .map_err(|e| error("internal", Code::Internal, e.to_string()))??;

    let input_shared = shared.clone();
    tokio::spawn(async move {
        let _guard = guard;
        while let Ok(Some(input)) = inbound.message().await {
            // A malformed input is dropped, like a corrupted radio packet.
            let Ok(sticks) = parse_sticks(input.sticks.unwrap_or_default()) else { continue };
            let s = input_shared.clone();
            let applied = tokio::task::spawn_blocking(move || {
                if let Some(session) = s.lock().as_mut() {
                    session.vehicle.set_sticks(&sticks);
                }
            })
            .await;
            if applied.is_err() {
                break;
            }
        }
    });
    Ok(state_feed(shared, rate_hz))
}

/// Streams session events. When the last watcher goes away, a watched session without keep_alive ends.
pub async fn watch(shared: Arc<Shared>) -> ReceiverStream<Result<pb::Event, Status>> {
    let mut events = shared.events.subscribe();
    shared.watchers.fetch_add(1, Ordering::AcqRel);
    let s = shared.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Some(session) = s.lock().as_mut() {
            session.watched = true;
        }
    })
    .await;
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                received = events.recv() => match received {
                    Ok(e) => {
                        if tx.send(Ok(e)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = tx.closed() => break,
            }
        }
        let _ = tokio::task::spawn_blocking(move || unwatch(&shared)).await;
    });
    ReceiverStream::new(rx)
}

fn unwatch(shared: &Shared) {
    if shared.watchers.fetch_sub(1, Ordering::AcqRel) != 1 {
        return;
    }
    let mut slot = shared.lock();
    let Some(s) = slot.as_ref() else { return };
    if !s.watched || s.keep_alive {
        return;
    }
    let t = s.vehicle.time_s();
    *slot = None; // stops the vehicle, and Betaflight SITL with it
    drop(slot);
    shared.publish([event(t, pb::EventKind::SessionEnded, "the last watcher disconnected and keep_alive is off")]);
}
```

In `crates/ofs-sim/src/lib.rs`, add `pub mod streams;` after `pub mod session;`. In `crates/ofs-sim/src/server.rs`:
1. Add `use crate::streams;` after `use crate::session::…;`.
2. Replace the three stub bodies:
   - `stream_state`:

```rust
        let rate_hz = streams::state_rate(req.into_inner().rate_hz)?;
        Ok(Response::new(streams::state_feed(self.shared.clone(), rate_hz)))
```

   - `pilot`:

```rust
        streams::pilot(self.shared.clone(), req.into_inner()).await.map(Response::new)
```

   - `watch`:

```rust
        Ok(Response::new(streams::watch(self.shared.clone()).await))
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --workspace`
Expected: 14 grpc tests and everything else pass, no warnings.

Run `cargo test -p ofs-sim --test grpc` 3 times: it passes every time.

- [ ] **Step 4: Commit**

```bash
git add crates/ofs-sim
git commit -m "feat(sim): state streams, the pilot transmitter link and session watchers"
```

---

### Task 13: Python client for protocol 2

**Files:**
- Replace: `python/ofs/client.py`, `python/ofs/__init__.py`
- Create:
  - `python/ofs/faults.py`;
  - `python/tests/test_realtime.py`, `python/tests/test_sitl_radio.py`;
  - `python/examples/serve_realtime.py`
- Modify: `python/ofs/errors.py`; `python/ofs/v1/` (regenerated stubs)

**Interfaces:**
- **Consumes:** protocol 2 (Tasks 11 and 12).
- **Produces:**
  - `ofs.PROTOCOL_VERSION = 2` (in `ofs.client`);
  - `Sim.load(quad, seed=0, mode="lockstep"|"realtime", open_loop_fc=False, overrun_policy="warn"|"slow", keep_alive=False)`;
  - `Sim.configurator_address`;
  - `Sim.start()`, `pause()`, `stream_states(rate_hz=60)` (generator), `inject(fault)`, `clear_faults()`, `events() -> list[Event]`;
  - `State.{radio: RadioLink, running, overruns, fc_restarts}`, with defaults so older constructors still work;
  - `ofs.RadioLink`, `ofs.Event(time_s, kind, message)`, where `kind` is the lower-case event name without the `EVENT_KIND_` prefix;
  - `ofs.faults.RadioLinkLoss()`;
  - `ofs.InvalidState`.
- **Behaviour:** the client opens a `Watch` stream on its first `load`. `close()` cancels it, which ends the session unless it was loaded with `keep_alive`.

- [ ] **Step 1: Write the failing tests.** Create `python/tests/test_realtime.py`:

```python
"""Protocol 2 features without firmware: real-time sessions, streams, faults, events and session lifetime."""
import time

import pytest

import ofs
from conftest import QUAD


def wait_for(predicate, timeout_s=3.0):
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return False


def test_realtime_session_follows_the_wall_clock(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    assert sim.state().time_s == 0.0 and not sim.state().running
    sim.start()
    time.sleep(1.0)
    s = sim.state()
    assert s.running
    assert 0.6 <= s.time_s <= 1.4, s.time_s
    sim.pause()
    paused_at = sim.state().time_s
    time.sleep(0.3)
    assert sim.state().time_s == paused_at
    with pytest.raises(ofs.InvalidState):
        sim.load(QUAD, open_loop_fc=True)  # lockstep
        sim.start()


def test_stream_states_yields_increasing_times(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.start()
    times = []
    for s in sim.stream_states(rate_hz=50):
        times.append(s.time_s)
        if len(times) == 5:
            break
    assert times == sorted(times)
    with pytest.raises(ofs.InvalidArgument):
        next(sim.stream_states(rate_hz=1000))


def test_radio_link_loss_fault_and_events(sim):
    sim.load(QUAD, open_loop_fc=True)
    assert sim.run(0.5).radio.link_up
    sim.inject(ofs.faults.RadioLinkLoss())
    assert not sim.run(0.5).radio.link_up
    sim.clear_faults()
    s = sim.run(0.5)
    assert s.radio.link_up and s.radio.lq_pct > 0
    kinds = []
    assert wait_for(lambda: kinds.extend(e.kind for e in sim.events()) or kinds.count("link_up") == 2), kinds
    assert kinds == ["link_up", "link_down", "link_up"]


def test_closing_a_client_ends_its_session(sim):
    other = ofs.connect(sim.address)
    other.load(QUAD, open_loop_fc=True)
    sim.state()  # one session per server: visible to every client
    other.close()

    def unloaded():
        try:
            sim.state()
            return False
        except ofs.NotLoaded:
            return True

    assert wait_for(unloaded), "the session outlived its client"


def test_keep_alive_sessions_outlive_their_client(sim):
    other = ofs.connect(sim.address)
    other.load(QUAD, open_loop_fc=True, keep_alive=True)
    other.close()
    time.sleep(0.5)
    sim.state()
```

Create `python/tests/test_sitl_radio.py`:

```python
"""Radio failsafe through the API against Betaflight SITL (spec §8.3). Needs OFS_SITL_LAUNCH."""
import os

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

ARM_AND_ANGLE = (1.0, 1.0, -1.0, -1.0)


def test_radio_cut_disarms_on_betaflight_failsafe_timing(sim):
    sim.load(QUAD, seed=1)
    assert sim.configurator_address == "tcp://127.0.0.1:5761"
    sim.run(4.0)  # boot and gyro calibration
    sim.set_sticks(aux=ARM_AND_ANGLE)
    s = sim.run(1.0)
    assert min(s.motor_cmd) > 0.0, f"never armed: {s.motor_cmd}"
    sim.inject(ofs.faults.RadioLinkLoss())
    cut = s.time_s
    while s.time_s < cut + 4.0 and max(s.motor_cmd) > 0.0:
        s = sim.run(0.01)
    assert max(s.motor_cmd) == 0.0, "Betaflight never disarmed"
    assert 1.4 <= s.time_s - cut <= 2.2, s.time_s - cut
    assert not s.radio.link_up
```

Run: `cargo build -p ofs-sim && python -m pytest python/tests -v`
Expected: failures. The client speaks protocol 1, so `ProtocolMismatch` is raised everywhere.

- [ ] **Step 2: Regenerate the stubs:**

```bash
python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto
```

Check that the stubs still require grpcio 1.84 and protobuf 7.35.1:
- `grep "GRPC_GENERATED_VERSION =" python/ofs/v1/sim_pb2_grpc.py`;
- `head -5 python/ofs/v1/sim_pb2.py`.

If they need newer versions, raise the floors in `python/pyproject.toml` to match.

- [ ] **Step 3: Implement.** Replace `python/ofs/client.py` with:

```python
"""Client for the ofs-sim gRPC server."""
from __future__ import annotations

import collections
import math
import os
import shutil
import socket
import subprocess
import threading
import time
from dataclasses import dataclass, field

import grpc

from ofs.v1 import sim_pb2 as pb
from ofs.v1 import sim_pb2_grpc as pbg

from .errors import ProtocolMismatch, from_rpc_error

PROTOCOL_VERSION = 2
UNLOAD_TIMEOUT_S = 5.0

_MODES = {"lockstep": pb.MODE_LOCKSTEP, "realtime": pb.MODE_REALTIME}
_POLICIES = {"warn": pb.OVERRUN_POLICY_WARN, "slow": pb.OVERRUN_POLICY_SLOW}


@dataclass(frozen=True)
class RadioLink:
    tx_enabled: bool = False
    link_up: bool = False
    lq_pct: float = 0.0
    rssi_dbm: float = 0.0


@dataclass(frozen=True)
class State:
    time_s: float
    position_ned_m: tuple
    velocity_ned_mps: tuple
    attitude_wxyz: tuple
    rate_frd_radps: tuple
    battery_voltage_v: float
    battery_current_a: float
    motor_rpm: tuple
    motor_cmd: tuple
    radio: RadioLink = field(default_factory=RadioLink)
    running: bool = False
    overruns: int = 0
    fc_restarts: int = 0

    @property
    def altitude_m(self) -> float:
        return -self.position_ned_m[2]

    def euler_deg(self) -> tuple:
        """(roll, pitch, yaw) in degrees, aerospace Z-Y-X convention, FRD body / NED world."""
        w, x, y, z = self.attitude_wxyz
        roll = math.degrees(math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)))
        pitch = math.degrees(math.asin(max(-1.0, min(1.0, 2 * (w * y - z * x)))))
        yaw = math.degrees(math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)))
        return roll, pitch, yaw


@dataclass(frozen=True)
class Event:
    time_s: float
    kind: str  # e.g. "link_down", "firmware_restarted", "overrun", "session_ended"
    message: str


def _v(m) -> tuple:
    return (m.x, m.y, m.z)


def _state(m) -> State:
    return State(
        time_s=m.time_s,
        position_ned_m=_v(m.position_ned_m),
        velocity_ned_mps=_v(m.velocity_ned_mps),
        attitude_wxyz=(m.attitude.w, m.attitude.x, m.attitude.y, m.attitude.z),
        rate_frd_radps=_v(m.rate_frd_radps),
        battery_voltage_v=m.battery_voltage_v,
        battery_current_a=m.battery_current_a,
        motor_rpm=tuple(m.motor_rpm),
        motor_cmd=tuple(m.motor_cmd),
        radio=RadioLink(m.radio.tx_enabled, m.radio.link_up, m.radio.lq_pct, m.radio.rssi_dbm),
        running=m.running,
        overruns=m.overruns,
        fc_restarts=m.fc_restarts,
    )


def _event(m) -> Event:
    name = pb.EventKind.Name(m.kind).removeprefix("EVENT_KIND_").lower()
    return Event(time_s=m.time_s, kind=name, message=m.message)


class Sim:
    """A connection to one ofs-sim server (and the process, if launched by `launch`).

    A session this client loads ends when the client disconnects (`close()`, or the script exits), unless it
    was loaded with `keep_alive=True`.
    """

    def __init__(self, address: str, process: subprocess.Popen | None = None):
        self.address = address
        self.configurator_address = ""
        self._process = process
        self._channel = grpc.insecure_channel(address)
        self._stub = pbg.SimStub(self._channel)
        self._watch = None
        self._events = collections.deque(maxlen=10_000)
        try:
            reply = self._call(self._stub.Handshake, pb.HandshakeRequest(protocol_version=PROTOCOL_VERSION))
            if reply.protocol_version != PROTOCOL_VERSION:
                raise ProtocolMismatch(f"server speaks protocol {reply.protocol_version}, client {PROTOCOL_VERSION}")
        except Exception:
            self._channel.close()
            raise

    @staticmethod
    def _call(method, request):
        try:
            return method(request)
        except grpc.RpcError as e:
            raise from_rpc_error(e) from None

    def _ensure_watch(self) -> None:
        """Opens the event stream (once). It also tells the server this client is alive."""
        if self._watch is not None:
            return
        self._watch = self._stub.Watch(pb.Empty())

        def pump(stream=self._watch, events=self._events):
            try:
                for m in stream:
                    events.append(_event(m))
            except grpc.RpcError:
                pass  # cancelled by close(), or the server went away

        threading.Thread(target=pump, name="ofs-watch", daemon=True).start()

    def load(self, quad_path: str, seed: int = 0, mode: str = "lockstep", open_loop_fc: bool = False,
             overrun_policy: str = "warn", keep_alive: bool = False) -> str:
        """Loads a quad. `mode` is "lockstep" (advance with `run`) or "realtime" (loads paused; `start` paces it
        to the wall clock). Returns the quad's name; `configurator_address` is set when it runs Betaflight."""
        if mode not in _MODES:
            raise ValueError(f"mode must be one of {sorted(_MODES)}")
        if overrun_policy not in _POLICIES:
            raise ValueError(f"overrun_policy must be one of {sorted(_POLICIES)}")
        self._ensure_watch()
        reply = self._call(self._stub.Load, pb.LoadRequest(
            quad_path=str(quad_path), seed=seed, mode=_MODES[mode], open_loop_fc=open_loop_fc,
            overrun_policy=_POLICIES[overrun_policy], keep_alive=keep_alive))
        self.configurator_address = reply.configurator_address
        return reply.quad_name

    def set_sticks(self, roll=0.0, pitch=0.0, yaw=0.0, throttle=0.0, aux=(-1.0, -1.0, -1.0, -1.0)) -> None:
        """Replaces the *whole* stick state: every argument left out goes back to its default.

        Sticks are in [-1, 1] (throttle in [0, 1]); `aux` holds up to 4 channels, missing ones read -1.
        The defaults include `aux=(-1, -1, -1, -1)`, so after arming, `set_sticks(throttle=0.6)` drops the
        arm switch and **disarms** the quad. Pass the aux channels on every call, e.g.
        `sim.set_sticks(throttle=0.6, aux=(1.0, 1.0, -1.0, -1.0))`.
        """
        self._call(self._stub.SetSticks, pb.Sticks(roll=roll, pitch=pitch, yaw=yaw, throttle=throttle, aux=list(aux)))

    def run(self, seconds: float) -> State:
        """Steps a lockstep session (or a paused real-time one) by `seconds` of simulated time."""
        return _state(self._call(self._stub.Run, pb.RunRequest(seconds=seconds)))

    def start(self) -> None:
        """Real-time sessions: run paced to the wall clock."""
        self._call(self._stub.Start, pb.Empty())

    def pause(self) -> None:
        self._call(self._stub.Pause, pb.Empty())

    def state(self) -> State:
        return _state(self._call(self._stub.GetState, pb.Empty()))

    def stream_states(self, rate_hz: int = 60):
        """Yields the state at `rate_hz` (1..240) until you stop iterating or the session ends."""
        call = self._stub.StreamState(pb.StreamRequest(rate_hz=rate_hz))
        try:
            for m in call:
                yield _state(m)
        except grpc.RpcError as e:
            if e.code() != grpc.StatusCode.CANCELLED:
                raise from_rpc_error(e) from None
        finally:
            call.cancel()

    def inject(self, fault) -> None:
        """Activates a fault from `ofs.faults` until `clear_faults()`."""
        self._call(self._stub.InjectFault, fault._to_pb())

    def clear_faults(self) -> None:
        self._call(self._stub.ClearFaults, pb.Empty())

    def events(self) -> list:
        """Events received since the last call (radio link up/down, firmware restarts, overruns, ...)."""
        out = []
        while self._events:
            out.append(self._events.popleft())
        return out

    def close(self) -> None:
        """Disconnects (ending this client's session unless it was loaded with keep_alive). For a server started
        by `launch`, first unloads the quad and then stops the server.

        Unloading stops Betaflight SITL cleanly; terminating the server alone would orphan it on Windows
        (TerminateProcess skips the server's cleanup).
        """
        if self._watch is not None:
            self._watch.cancel()
            self._watch = None
        if self._process is not None:
            try:
                self._stub.Unload(pb.Empty(), timeout=UNLOAD_TIMEOUT_S)
            except Exception:  # best effort: the server may already be gone or failing
                pass
        self._channel.close()
        if self._process is not None:
            self._process.terminate()
            try:
                self._process.wait(5)
            except subprocess.TimeoutExpired:
                self._process.kill()
            self._process = None

    def __enter__(self) -> "Sim":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


def connect(address: str = "127.0.0.1:50051") -> Sim:
    return Sim(address)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def launch(binary: str | None = None, headless: bool = True, data_dir: str = ".ofs-data",
           timeout_s: float = 15.0) -> Sim:
    """Starts an ofs-sim server on a free local port and connects to it."""
    if not headless:
        raise NotImplementedError("the Godot pilot client arrives with M2b; only headless=True is available")
    binary = binary or os.environ.get("OFS_SIM_BIN") or shutil.which("ofs-sim")
    if not binary:
        raise FileNotFoundError("ofs-sim binary not found: set OFS_SIM_BIN or put ofs-sim on PATH")
    address = f"127.0.0.1:{_free_port()}"
    proc = subprocess.Popen([binary, "--listen", address, "--data-dir", data_dir])
    deadline = time.monotonic() + timeout_s
    channel = grpc.insecure_channel(address)
    try:
        while True:
            if proc.poll() is not None:
                raise RuntimeError(f"ofs-sim exited with code {proc.returncode} during startup")
            try:
                grpc.channel_ready_future(channel).result(timeout=0.2)
                break
            except grpc.FutureTimeoutError:
                if time.monotonic() > deadline:
                    proc.kill()
                    raise RuntimeError(f"ofs-sim did not accept connections on {address} within {timeout_s} s")
    finally:
        channel.close()
    try:
        return Sim(address, proc)
    except BaseException:  # e.g. ProtocolMismatch: don't leave the server running
        proc.kill()
        proc.wait()
        raise
```

Create `python/ofs/faults.py`:

```python
"""Faults to inject with `Sim.inject` (spec §6.2). M2 ships the radio link loss; the v1 catalog completes in M4."""
from dataclasses import dataclass

from ofs.v1 import sim_pb2 as pb


@dataclass(frozen=True)
class RadioLinkLoss:
    """Every radio uplink packet is lost while active: the receiver goes silent and Betaflight fails safe."""

    def _to_pb(self):
        return pb.Fault(radio_link_loss=pb.RadioLinkLoss())
```

In `python/ofs/errors.py`, add after `class InvalidArgument…`:

```python


class InvalidState(OfsError):
    """The call does not fit the session's mode or state (e.g. Start on a lockstep session)."""
```

and add `"invalid_state": InvalidState,` to `_KINDS` after `"invalid_argument": InvalidArgument,`.

Replace `python/ofs/__init__.py` with:

```python
"""Python client for Open FPV Sim."""
from . import faults
from .client import Event, RadioLink, Sim, State, connect, launch
from .errors import (ConfigError, FirmwareCrashed, InvalidArgument, InvalidState, NotLoaded, NumericalError, OfsError,
                     ProtocolMismatch, ServerUnavailable)

__all__ = [
    "Sim", "State", "RadioLink", "Event", "connect", "launch", "faults",
    "OfsError", "ConfigError", "FirmwareCrashed", "NumericalError", "ProtocolMismatch", "NotLoaded",
    "InvalidArgument", "InvalidState", "ServerUnavailable",
]
```

Create `python/examples/serve_realtime.py`:

```python
"""Runs the quad with Betaflight SITL in real time until Ctrl+C, so Betaflight Configurator can connect.

Usage (repo root): OFS_SITL_LAUNCH="<sitl launch command>" python python/examples/serve_realtime.py [quad.toml] [seconds]
Then in Betaflight Configurator (desktop app): enable manual connection, port tcp://127.0.0.1:5761, Connect.
Saving in the Configurator reboots Betaflight; the simulator relaunches it and the Configurator reconnects.
"""
import sys
import time

import ofs


def serve(quad: str, seconds: float = float("inf")) -> ofs.State:
    with ofs.launch() as sim:
        sim.load(quad, seed=1, mode="realtime")
        sim.start()
        print(f"running in real time; Betaflight Configurator: {sim.configurator_address} (Ctrl+C to stop)", flush=True)
        started = time.monotonic()
        s = sim.state()
        try:
            while time.monotonic() - started < seconds:
                time.sleep(1.0)
                s = sim.state()
                link = "up" if s.radio.link_up else "DOWN"
                print(f"t={s.time_s:7.1f} s  radio {link}  overruns={s.overruns}  betaflight restarts={s.fc_restarts}",
                      flush=True)
        except KeyboardInterrupt:
            pass
        return s


if __name__ == "__main__":
    quad = sys.argv[1] if len(sys.argv) > 1 else "quads/opendrone-5f-freestyle.toml"
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else float("inf")
    serve(quad, seconds)
```

- [ ] **Step 4: Run the tests**

Run: `cargo build -p ofs-sim && python -m pytest python/tests -v`
Expected: 12 passed, 3 skipped (the SITL tests).

Then with `OFS_SITL_LAUNCH` set:

```bash
python -m pytest python/tests -v
```

Expected: 15 passed.

Run `python python/examples/serve_realtime.py quads/opendrone-5f-freestyle.toml 8`. It prints 8 lines like `t=    3.0 s  radio up  overruns=0  betaflight restarts=0`, and no SITL is left afterwards.

- [ ] **Step 5: Commit**

```bash
git add python
git commit -m "feat(python): protocol 2 client: real-time sessions, streams, faults, events"
```

---

### Task 14: Docs, CI, research notes, and the Configurator check

**Files:**
- Replace: `docs/dev-setup.md`, `README.md`, `.github/workflows/ci.yml`
- Modify: `docs/research/sitl-interface.md` (new §8), `docs/superpowers/m1-carried-debt.md`

**Interfaces:** documents Tasks 1–13. The CI `sitl` job runs every live test, and a new `msrv` job checks the declared `rust-version`.

- [ ] **Step 1: Replace** `docs/dev-setup.md` with:

````markdown
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
| Real-time session (for Configurator) | `OFS_SITL_LAUNCH=<cmd> python python/examples/serve_realtime.py` |
| Regenerate Python stubs | `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` |
| Regenerate the SITL patch | see the docstrings in `third_party/betaflight/tools/` |

## Environment variables
- `OFS_SIM_BIN` — path to `ofs-sim` used by `ofs.launch()`.
- `OFS_SITL_LAUNCH` — SITL argv (space-separated, so no spaces inside paths), overrides `fc.launch` in quad files.
  Windows: `wsl.exe -d Ubuntu -e /home/<user>/ofs/betaflight/obj/main/betaflight_SITL.elf`. Works with WSL's default NAT networking: the bridge discovers the WSL VM and host IPs and passes `--ip` (see `docs/research/sitl-interface.md` §6).
- `OFS_SITL_CLEANUP` — argv run before launch and after stop to kill stray SITL processes (also when a native SITL's UDP 9003 is still held). It defaults to `pkill -x betaflight_SITL` on Linux and to `<wsl prefix> pkill -x betaflight_SITL` under WSL (required there: a stale SITL would otherwise answer instead of the new one). It matches the process name: never use `pkill -f`, which also matches the shell running it.
- `OFS_SITL_HOST`, `OFS_SITL_REPLY_IP` — override the address state datagrams go to, and the address SITL replies to (`--ip`, motor socket bind).

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
3. Saving reboots Betaflight; the simulator relaunches it and the Configurator reconnects.

The web Betaflight App cannot open raw TCP connections.

## Ports
SITL uses fixed ports (UDP 9001–9004, TCP 5760+), so only one SITL session can run per machine and SITL tests must not run in parallel.
````

- [ ] **Step 2: Replace** `README.md` with:

````markdown
# Open FPV Sim

An open-source FPV drone simulator that runs **real Betaflight** (SITL) against physics, electrical, sensor and radio models,
replicating real protocols so real tools work against it. Inspired by the [OpenDrone](https://opendrone.be/) open-hardware initiative.

Status: **M2a — radio link and real time.**
- Betaflight flies through a simulated ExpressLRS/CRSF link and fails safe on link loss.
- Sessions run in lockstep (deterministic, Betaflight included) or paced to the wall clock.
- Betaflight Configurator can connect.
- Next is M2b: the Godot pilot client.

- Design: `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`
- SITL interface findings: `docs/research/sitl-interface.md`
- Developer setup: `docs/dev-setup.md`

## Quick start (open loop, no firmware)

    cargo build -p ofs-sim
    python -m pip install -e "python[dev]"
    python -c "import ofs; s = ofs.launch(binary='target/debug/ofs-sim'); s.load('quads/opendrone-5f-freestyle.toml', open_loop_fc=True); s.set_sticks(throttle=0.6); print(s.run(1.0)); s.close()"

## With real Betaflight (from the repository root)

    bash scripts/build-sitl.sh                      # Linux or WSL2
    export OFS_SIM_BIN=target/debug/ofs-sim
    export OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf   # Windows: see docs/dev-setup.md
    python python/examples/hover.py                 # lockstep: arm and hold 1 m
    python python/examples/serve_realtime.py        # real time; connect Betaflight Configurator to tcp://127.0.0.1:5761

License: GPL-3.0-or-later.
````

- [ ] **Step 3: Append §8 to** `docs/research/sitl-interface.md`:

````markdown

## 8. M2 changes (radio link, real time, reboots)

### UART bytes in the state datagram
- **Format.** After the 184-byte datagram the simulator appends blocks of `[uart index (0-based)][len lo][len hi][bytes]`, at most 512 bytes in total (`EXT_SERIAL_MAX`). SITL's FDM thread stages them with the packet. `simulatorTakeGyroTick()` hands them to `tcpSerialInject()` → the port's RX buffer → the driver at that tick's `tcpSerialDispatchRx()`. The receiver's CRSF output therefore reaches Betaflight on a deterministic tick. M0 §5's caveat for TCP UARTs still applies to real tools connected over TCP.
- **Generator:** `third_party/betaflight/tools/add_serial_in_datagram.py`.
- **Verified:** CRSF sticks roll 0.2, pitch −0.2, yaw 0, throttle 0 read back over MSP_RC as exactly 1600 / 1400 / 1500 / 1000 µs.
- **UDP RC:** the `rc_packet` channels are sent as zeros. With `serialrx_provider = CRSF`, Betaflight ignores them. A stale EEPROM still on the UDP receiver sees invalid pulses and fails safe.

### Deterministic boot
- **Symptom.** With CRSF, about 40 % of compared lockstep flights diverged, always at the same instant. Motor 0 read 0.245 vs 0.246 two seconds after arming. The M1 path (RC inside the datagram) stayed deterministic on the same binary.
- **Ruled out.** The divergence was independent of packet rate (500 Hz / 1 kHz) and of link-statistics frames. `eeprom.bin` was identical before every flight.
- **Located.** A Blackbox recorded from boot (`blackbox_mode = ALWAYS`) showed the attitude estimate already differing by 1–2 counts 0.3 s after the first packet. The recording also started 1 ms apart.
- **Cause.** SITL's UDP thread starts early in `systemInit()`, so a state packet could land while init still ran. Init read the fake sensors before or after the packet's values arrived. CRSF's longer init moved which side of that race boots landed on. Separately, the scheduler ran tasks on wall time until the first packet.
- **Fix** (`third_party/betaflight/tools/deterministic_boot.py`):
  - state packets are ignored until the scheduler runs;
  - SITL prints `[SITL] ready for the simulator` at that moment;
  - the scheduler runs no task before the first packet;
  - the bridge waits for that line instead of probing TCP 5761 and sleeping 300 ms.
- **Result:** 8/8 compared flights bit-identical; the M1-path determinism test still passes.

### Reboots
- **What SITL does.** `systemReset()` prints `[system]Reset!`, joins its threads and calls `exit(0)`. Before that, `motorShutdown()` sleeps 501.5 ms of real time (PWM protocol), longer than the 500 ms reply timeout.
- **What the bridge does.** After a missed reply it waits up to 3 s for "exited 0 and the reset line seen". It then drops the old process (its `pkill` cleanup would kill a new instance), relaunches from the same EEPROM with the same argv, and resends the current datagram with the first-exchange resend logic. SITL's clock restarts from the next packet (fixed 10 s base).
- **Live test:** `a_betaflight_reboot_is_a_firmware_restart_not_a_crash`.

### Real time and MSP
- In a real-time session on Windows + WSL2, SITL answered `MSP_API_VERSION` (API 1.48) in about 16 ms without the client stepping anything.
- An 8 s real-time run had 0 overruns.

### Failsafe timing
- **Measured.** With the radio link cut while armed at idle, Betaflight disarmed 1.4–2.2 s later, consistent with its code:
  - frames stop;
  - `failsafe_delay` defaults to 1.5 s (`failsafeOnValidDataFailed`);
  - then `JUST_DISARM` at low throttle, or `DROP`.
- **Live test:** `a_radio_cut_fails_safe_on_betaflight_timing`.
````

- [ ] **Step 4: Update** `docs/superpowers/m1-carried-debt.md`. Replace the "Server and session lifecycle" section with:

```markdown
## Server and session lifecycle
- **Resolved in M2a:**
  - graceful `ofs-sim` shutdown on Ctrl-C/SIGTERM;
  - `Run` stops within 50 ms when its client goes away;
  - a Python client's session ends when the client disconnects (unless `keep_alive`);
  - firmware directories are keyed on the quad file's path.
- **Non-loopback `--listen` (still open).** `Load` reads any path and executes that quad file's `fc.launch` argv, and TOML parse errors echo file contents back. Refuse or warn on non-loopback binds unless an explicit flag is given, before M2b exposes the server to more clients.
```

In the "SITL bridge" section, replace the bullet that starts "**Stray second reply after a resend.**" with:

```markdown
- **Stray second reply after a resend.** If SITL received both copies of a resent first datagram, the second reply could arrive after the next drain and leave a one-tick lag. This is now rare: SITL ignores packets until it is ready and the bridge waits for its ready line, so resends only happen on lost datagrams. A settle-and-drain after a resent first exchange would close it fully.
```

- [ ] **Step 5: Replace** `.github/workflows/ci.yml` with:

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
      - run: cargo check --workspace --all-targets --locked

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

- [ ] **Step 6: Verify everything.**
  1. Run on Windows: `cargo test --workspace`, then `cargo build -p ofs-sim && python -m pytest python/tests -v`.
  2. Repeat with `OFS_SITL_LAUNCH` set, plus the live Rust tests 3 times.
  3. Repeat on Linux (WSL native).
  4. After pushing the branch, check that all four CI jobs pass.

  If `msrv` fails because a locked dependency needs a newer Rust, raise `rust-version` in `Cargo.toml` and the requirement in `docs/dev-setup.md` to the version the error names, and record the dependency in the commit message.

- [ ] **Step 7: The Configurator check (needs the user).** Ask the user to run `python python/examples/serve_realtime.py` (with `OFS_SITL_LAUNCH` set), connect Betaflight Configurator to `tcp://127.0.0.1:5761`, then:
  1. change one PID value and press Save;
  2. reconnect and confirm the value persisted;
  3. note `betaflight restarts=1` in the script's output.

  Record the result (Configurator version, pass/fail, observations) in `docs/research/sitl-interface.md` §8 under "Real time and MSP". This closes the M0 open item. If the user cannot run it now, record "pending" and leave the item open in the carried-debt doc.

- [ ] **Step 8: Commit**

```bash
git add README.md docs .github/workflows/ci.yml
git commit -m "docs: M2a radio link, real time and Configurator; CI runs all live tests and checks MSRV"
```
