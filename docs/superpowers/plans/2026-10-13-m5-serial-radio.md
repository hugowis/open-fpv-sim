# M5: the real-radio bridge — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A real ELRS handset or TX module (serial CRSF) flies the simulator through the existing link model, and
its handset shows the simulated link's telemetry.

**Architecture:** `ofs-sim` owns the serial port behind a `SerialTransport` trait (real `serialport`, in-memory
duplex for tests, pty for Linux end to end). The bridge thread decodes CRSF with the existing `ofs_radio::crsf`
codec and applies channels through `Vehicle::set_sticks` — the same path and guards as the RPC. Telemetry answers
are built from the vehicle's own state snapshot and encoded by new pure functions in `crsf.rs`.

**Tech Stack:** Rust 1.85 workspace (`serialport` 4.x is the only new dependency, in `ofs-sim`), tonic/prost proto
(version 10), Python 3.10+ (`pyserial` for the capture tool and the pty fake handset), Godot 4.7 with gdext.

**Spec:** `docs/superpowers/specs/2026-10-13-m5-serial-radio-design.md` (the plan argues from it; read both).
Phase parent: `docs/superpowers/specs/2026-10-13-phase2-real-gear-trusted-numbers-design.md`.

## Global Constraints

- Branch `m5-serial-radio`, cut from `main` **after the v1 close-out merges**. Commit per task; never push to
  `main`; never tag.
- Commit messages: the repository's `type(scope): sentence` style, ending with the line
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Protocol version **10** (`crates/ofs-proto/src/lib.rs` and `python/ofs/client.py` both move 9 → 10).
- Rust: `cargo clippy --workspace --all-targets --locked -- -D warnings` stays clean; MSRV 1.85.
- Every Rust/Python behaviour change is test-first: the failing test, its failure seen, then the fix.
- `ofs-radio` and `ofs-core` stay I/O-free (the `Model` trait rule): no `serialport` dependency outside `ofs-sim`.
- Draw discipline (M4b ruling 7): any new random-looking branch still consumes draws in fixed positions; the
  pass-through flag changes which results are used, never how many are drawn.
- **Capture wins:** the wire constants in this plan (`DEFAULT_BAUD` 420000, AETR channel order, 10 Hz telemetry,
  the frame set) are the *expected* values, to be confirmed by Task 1's capture. Where the capture disagrees, the
  capture's value replaces the plan's, and the commit message names the difference.
- Live SITL tests: one SITL per machine (fixed ports), never in parallel (`--test-threads=1`).
- Docs: plain, factual sentences in the existing docs' voice; every claim about behaviour names its test or the
  manual check that showed it.

### How to run things (this machine)

- **Windows Rust:** from `C:\dev\open-fpv-sim`: `cargo test --workspace --locked`, then the clippy line above.
- **Windows Python (no SITL):** `python -m pytest python/tests -q` (after `cargo build -p ofs-sim`). The pty
  tests skip (`posix` markers) — Windows covers them by review of the code and the maintainer's manual session.
- **WSL live (Rust):** from `/mnt/c/dev/open-fpv-sim`:
  `CARGO_TARGET_DIR=$HOME/ofs/m5-exec-target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf ~/.cargo/bin/cargo test -p ofs-fc -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`
- **WSL live (Python):** after `CARGO_TARGET_DIR=$HOME/ofs/m5-exec-target ~/.cargo/bin/cargo build -p ofs-sim --locked`:
  `OFS_SIM_BIN=$HOME/ofs/m5-exec-target/debug/ofs-sim OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf ~/.local/bin/uv run --no-project --with-editable './python[dev]' python -m pytest -q <files>`
- **Godot:** `GODOT_BIN=C:/tools/godot/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit`
  (or `e2e`); the script builds `ofs-sim` and `ofs-godot` itself.
- Use the Write tool for any script that contains backslashes (Bash heredocs collapse `\\`).

## Review Focus

1. **A noisy or corrupt byte stream at full rate** (a flapping cable, a baud mismatch): the decoder skips to sync,
   CRC errors are counted per bridge lifetime, the sim never stalls and no buffer grows — Task 6 pins both
   (`garbage_bytes_never_stall_or_grow_the_bridge`, the running CRC count).
2. **Tick values outside the calibration** (extended-servo values > 1811 or < 172 on a channel): clamped into
   `[-1, 1]` / `[0, 1]`, never NaN, never a value Betaflight would reject — Task 2 pins the clamp, Task 6 pins the
   mapping end to end.
3. **The cable pulled mid-flight while armed** (or the radio switched off): the bridge publishes
   `SERIAL_RADIO_LOST`, detaches, the transmitter goes off, and Betaflight failsafes on its normal schedule; a
   Godot pilot can connect immediately after — Task 6 (open loop), Task 11 (live SITL), Task 9 (event + link
   state).
4. **Attach/detach racing a pilot connect or a PlayInputs at the same moment:** exactly one stick owner ever
   holds the sticks, whichever request lands first — Task 5 pins the matrix (double attach, lockstep, and the
   attach-while-pilot gRPC test), Task 9 re-checks the RPC refusals from the Python side.
5. **A radio CSV recorded across a pause** (the radio keeps sending while the sim is paused): rows keep
   simulated `t_s` (non-decreasing, with repeats), and the file still parses and replays — Task 7 pins the
   format against the server's own `inputs::parse`, Task 10 replays one.

---

## File Structure

| File | Change | Task |
|---|---|---|
| `scripts/capture-serial-radio.py` (new) | the capture tool | 1 |
| `crates/ofs-radio/tests/fixtures/serial-radio/` (new) | capture fixtures (committed) | 1 |
| `docs/research/serial-radio.md` (new) | the wire's ground truth | 1, 12 |
| `crates/ofs-radio/src/crsf.rs` | reverse conversions; battery + flight-mode encoders | 2 |
| `crates/ofs-radio/tests/crsf_serial.rs` (new) | golden + conversion tests | 2 |
| `proto/ofs/v1/sim.proto` | protocol 10: rpcs, messages, event kinds | 3 |
| `crates/ofs-proto/src/lib.rs`, `python/ofs/client.py` | version bumps | 3 |
| `crates/ofs-sim/Cargo.toml`, `Cargo.toml` | `serialport` dependency | 4 |
| `crates/ofs-sim/src/radio_serial.rs` (new) | the bridge: transport seam, list, thread, recorder | 4, 5, 6, 7 |
| `crates/ofs-sim/src/session.rs` | `Shared.serial_radio`; `Session.serial_radio` field | 5 |
| `crates/ofs-sim/src/server.rs` | the three RPC handlers; refusals in Pilot/PlayInputs/SetSticks | 5 |
| `crates/ofs-radio/src/elrs.rs`, `crates/ofs-core/src/names.rs` | the `radio.passthrough` signal | 6 |
| `crates/ofs-radio/tests/elrs.rs` | pass-through tests | 6 |
| `crates/ofs-client/src/model.rs`, `src/worker.rs` | client Commands + Update for the game | 8 |
| `python/ofs/client.py`, `python/ofs/__init__.py` | `attach_radio`/`detach_radio`/`list_serial_ports` | 8 |
| `python/tests/test_client.py` | client-method tests (Linux: pty) | 8 |
| `python/tests/test_serial_radio.py` (new) | the fake handset end to end (posix) | 9 |
| `python/tests/test_recording.py` | radio-CSV replay determinism (posix) | 10 |
| `python/tests/test_sitl_radio.py` | live failsafe-on-unplug; pass-through smoke | 11 |
| `crates/ofs-godot/src/lib.rs` | `list_serial_ports`, `radio_attach`, `radio_detach` | 12 |
| `godot/ui/controls_menu.gd`, `godot/scripts/app.gd`, `godot/tests/test_controls_menu.gd` | the F2 Radio section | 12 |
| `README.md`, `docs/dev-setup.md` | radio-bridge docs | 12 |

---

### Task 1: The capture — tool, maintainer session, fixtures, research doc

Spec §2 (decision 8: ground truth first). Everything later reads the capture's constants; nothing in Tasks 2–12
may contradict it. **The maintainer's hands are part of this task** (the RadioMaster EdgeTX15); the tooling is
built first so the capture session is one guided run.

**Files:**
- Create: `scripts/capture-serial-radio.py`
- Create: `crates/ofs-radio/tests/fixtures/serial-radio/{capture.jsonl, summary.toml}`
- Create: `docs/research/serial-radio.md`

**Interfaces:**
- Produces: `crates/ofs-radio/tests/fixtures/serial-radio/capture.jsonl` — one JSON object per line:
  `{"t_ms": <int wall-clock ms since tool start>, "dir": "tx2host" | "host2tx", "hex": "<lowercase hex>"}`,
  one line per serial read/write, in order.
- Produces: `crates/ofs-radio/tests/fixtures/serial-radio/summary.toml` with this exact shape (the tool writes
  it; Tasks 2/6/9 assert against it):

```toml
baud = 420000                     # the port was opened at this rate
channel_order = "AETR"            # the ruling from the wiggle windows
telemetry_hz = 10.0               # measured host->tx reply cadence when the LUA script polls (0 = never seen)

[windows]                         # t_ms ranges of each guided phase
idle = [0, 10000]
roll = [10000, 20000]             # the maintainer moved ONLY the roll stick in this window
pitch = [20000, 30000]
yaw = [30000, 40000]
throttle = [40000, 50000]
switches = [50000, 60000]
telemetry_screen = [60000, 75000]
lua = [75000, 90000]

[frame_counts]                    # frame type (hex string) -> count, tx2host only
"0x16" = 12345
```

- [ ] **Step 1: Write the capture tool**

Create `scripts/capture-serial-radio.py` (stdlib + `pyserial`; document `python -m pip install pyserial` in its
docstring). It runs the guided session:

1. `--port` argument (or `--list` to print candidate ports with descriptions via `serial.tools.list_ports`).
2. Opens the port at `--baud` (default 420000), then runs the phases in order, printing one prompt per phase
   ("Phase roll: move ONLY the roll stick slowly end to end for 10 s…") and waiting a fixed duration per phase,
   stamping each phase's `[t_start_ms, t_end_ms]` into the windows table.
3. One reader thread appends `{"t_ms", "dir": "tx2host", "hex"}` lines to `capture.jsonl` per read; every write
   the tool itself makes (none by default) would be logged the same way.
4. On Ctrl+C or after the last phase: decode the capture with the same rules as the Rust codec (address byte in
   `[0xC8, 0xEA, 0xEC, 0xEE]`, length byte `2..=62`, CRC8-DVB-S2 — implement the CRC in the script; it is 8
   lines: poly 0xD5, init 0), count frame types into `frame_counts`, measure the median tx2host `0x16`
   (RC_CHANNELS) period, and measure the host→tx reply cadence if any host→tx lines exist.
5. Writes `summary.toml` and prints a human summary (frame table + periods) for the research doc.

The channel-order check is part of the tool: for each of the four wiggle windows it reports which CRSF channel
index spanned the widest tick range and prints the inferred order. The maintainer confirms it against what they
moved (the tool asks "did that match roll→ch0, pitch→ch1, yaw→ch2, throttle→ch3? [y/n]" and writes
`channel_order` from the answer).

- [ ] **Step 2: The maintainer capture session**

With the maintainer: plug the handset into USB, put the internal ELRS module into simulator mode (EdgeTX: the
ELRS LUA script's **[Sim]** button, or Model Setup → Internal RF → ExpressLRS → simulator mode — the module then
enumerates as a USB CDC port), then:

```bash
python scripts/capture-serial-radio.py --list      # find the port (COM7 / /dev/ttyACM0)
python scripts/capture-serial-radio.py --port COM7 --out crates/ofs-radio/tests/fixtures/serial-radio
```

Follow the prompts (idle; roll only; pitch only; yaw only; throttle only; switches; open EdgeTX's telemetry
screen; open the ELRS LUA script). If the radio offers a baud setting, leave it at its default and record what
the connection used. Copy the tool's printed summary into the research doc (Step 3). If simulator mode does not
produce a serial port, stop and report — the spec's §2 capture-first ruling means M5 does not proceed on
assumptions.

- [ ] **Step 3: Write `docs/research/serial-radio.md`**

Sections, each stating a fact from the capture (date + port + handset model named at the top):

- **Transport** — baud, parity/stop bits, how the port enumerates on Windows (device description) and Linux
  (path + the `dialout` group note).
- **Frames from the handset** — the frame-type table with counts and measured periods; which type carries
  channels; whether the handset transmits continuously or idles un-armed (this decides whether the bridge's
  `set_transmitter(true)` fires immediately or on arming).
- **Channel order** — the wiggle-window verdict (AETR or otherwise) and the aux channel that carried a switch.
- **Telemetry** — what the telemetry screen and the LUA script read, and whether the capture saw any host→tx
  traffic at all (expected: none from EdgeTX itself; the bridge supplies it).
- **Rulings** — `DEFAULT_BAUD`, the channel order, the telemetry cadence M5 will use, each with the capture as
  its source. State plainly anything the capture did not show.

- [ ] **Step 4: Commit**

```bash
git add scripts/capture-serial-radio.py crates/ofs-radio/tests/fixtures/serial-radio docs/research/serial-radio.md
git commit -m "feat(radio): the serial-radio capture - tool, fixtures, and the wire's ground truth

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 2: CRSF reverse conversions and telemetry encoders (`crsf.rs`)

Spec §3 input path, §4 telemetry path. Pure functions, golden-tested against the capture and hand-computed bytes.

**Files:**
- Modify: `crates/ofs-radio/src/crsf.rs`
- Test: `crates/ofs-radio/tests/crsf_serial.rs` (new)

**Interfaces:**
- Produces: `crsf::ticks_to_stick(ticks: u16) -> f64` (inverse of `stick_ticks`, clamped to [-1, 1]);
  `crsf::ticks_to_throttle(ticks: u16) -> f64` (inverse of `throttle_ticks`, clamped to [0, 1]);
  `crsf::BatterySensor { voltage_mv: u16, current_ma: u16, capacity_used_mah: u32, remaining_pct: u8 }` and
  `crsf::battery_sensor_frame(&BatterySensor) -> Vec<u8>`; `crsf::flight_mode_frame(text: &str) -> Vec<u8>`
  (truncates to 16 bytes; empty text is a caller error — the bridge skips the frame instead);
  `crsf::telemetry_frame(frame_type: u8, payload: &[u8]) -> Vec<u8>` (address `0xEC` → the radio, length byte,
  payload, CRC — the shape the CRSF spec calls a telemetry frame; the capture's host→tx lines, if any, pin the
  address byte instead).

- [ ] **Step 1: Write the failing tests**

Create `crates/ofs-radio/tests/crsf_serial.rs`:

```rust
//! The serial-radio bridge's CRSF half: reverse conversions and the telemetry encoders (M5, spec §3-§4).
//! Golden facts come from the capture (Task 1); the telemetry frames are hand-computed from the CRSF spec
//! and confirmed by the maintainer's handset in the manual session.

use ofs_radio::crsf::{self, BatterySensor, Decoder, Frame, LINK_STATISTICS_PAYLOAD_SIZE};

fn capture_summary() -> toml::Table {
    let text = include_str!("fixtures/serial-radio/summary.toml");
    text.parse().expect("the capture summary parses")
}

fn capture_frames() -> Vec<Frame> {
    let text = include_str!("fixtures/serial-radio/capture.jsonl");
    let mut decoder = Decoder::default();
    let mut frames = Vec::new();
    for line in text.lines() {
        let v: serde_json::Value = serde_json::from_str(line).expect("each capture line is JSON");
        if v["dir"].as_str() != Some("tx2host") {
            continue;
        }
        let hex = v["hex"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex.len()).step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        frames.extend(decoder.push(&bytes));
    }
    frames
}

#[test]
fn the_capture_decodes_mostly_into_rc_channels() {
    let frames = capture_frames();
    assert!(frames.len() > 100, "a real capture has thousands of frames, got {}", frames.len());
    let rc = frames.iter().filter(|f| matches!(f, Frame::RcChannels(_))).count();
    assert!(rc * 10 > frames.len() * 9, "RC_CHANNELS dominates the capture: {rc}/{}", frames.len());
}

#[test]
fn the_wiggle_windows_confirm_the_documented_channel_order() {
    // The capture's summary names the order; this test re-derives it from the frames so the fixture and the
    // doc cannot drift apart. Roll moved alone in its window: channel 0 swings wide, channel 1 stays near centre.
    let windows = capture_summary()["windows"].as_table().unwrap().clone();
    let roll = windows["roll"].as_array().unwrap();
    let (t0, t1) = (roll[0].as_integer().unwrap(), roll[1].as_integer().unwrap());
    let mut min_tick = [u16::MAX; 4];
    let mut max_tick = [0u16; 4];
    let mut checked = 0;
    let mut last_t = 0i64;
    for line in include_str!("fixtures/serial-radio/capture.jsonl").lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let t = v["t_ms"].as_i64().unwrap();
        if v["dir"].as_str() != Some("tx2host") || t < t0 || t > t1 { last_t = t; continue; }
        let hex = v["hex"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex.len()).step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        for f in Decoder::default().push(&bytes) {
            if let Frame::RcChannels(ch) = f {
                for (i, slot) in min_tick.iter_mut().zip(max_tick.iter_mut()).enumerate().take(4) {
                    *slot.0 = (*slot.0).min(ch[i]);
                    *slot.1 = (*slot.1).max(ch[i]);
                }
                checked += 1;
            }
        }
        last_t = t;
    }
    assert!(checked > 20, "the roll window holds RC frames");
    let span = |i: usize| max_tick[i].saturating_sub(min_tick[i]);
    assert!(span(0) > 1000, "channel 0 swings wide in the roll window: {:?}", &min_tick[..4]);
    assert!(span(1) < 300, "channel 1 holds still: {:?}", &min_tick[..4]);
}

#[test]
fn tick_conversions_round_trip_and_clamp() {
    for ticks in [192u16, 500, 992, 1500, 1792] {
        assert_eq!(crsf::stick_ticks(crsf::ticks_to_stick(ticks)), ticks, "{ticks}");
    }
    for ticks in [192u16, 1000, 1792] {
        assert_eq!(crsf::throttle_ticks(crsf::ticks_to_throttle(ticks)), ticks, "{ticks}");
    }
    // Extended-servo values clamp, never wrap or NaN.
    assert_eq!(crsf::ticks_to_stick(0), -1.0);
    assert_eq!(crsf::ticks_to_stick(1811), 1.0);
    assert_eq!(crsf::ticks_to_stick(u16::MAX), 1.0);
    assert_eq!(crsf::ticks_to_throttle(0), 0.0);
    assert_eq!(crsf::ticks_to_throttle(u16::MAX), 1.0);
    assert!(crsf::ticks_to_stick(172).is_finite());
}

#[test]
fn a_battery_frame_encodes_the_crsf_battery_sensor_shape() {
    // CRSF battery sensor: type 0x08, payload voltage (u16, 10 mV), current (u16, 10 mA),
    // capacity (u24, mAh), remaining percent (u8).
    let frame = crsf::battery_sensor_frame(&BatterySensor {
        voltage_mv: 24_590, current_ma: 12_340, capacity_used_mah: 321, remaining_pct: 87,
    });
    assert_eq!(frame[0], 0xEC, "addressed to the radio");
    assert_eq!(frame[1], 10, "address + type + 8 payload bytes + crc");
    assert_eq!(frame[2], 0x08);
    assert_eq!(&frame[3..5], &[0x9B, 0x09], "24590 mV = 2459 * 10 mV = 0x099B, little-endian");
    assert_eq!(&frame[5..7], &[0xD2, 0x04], "12340 mA = 1234 * 10 mA = 0x04D2, little-endian");
    assert_eq!(&frame[7..10], &[0x41, 0x01, 0x00], "321 mAh as u24, little-endian");
    assert_eq!(frame[10], 87);
    assert_eq!(frame[11], crsf::crc8_dvb_s2(&frame[2..11]), "the crc covers type + payload");
    assert_eq!(Decoder::default().push(&frame), vec![Frame::Other { address: 0xEC, frame_type: 0x08, payload: frame[3..11].to_vec() }],
        "the sim's own decoder parses it back");
}

#[test]
fn a_flight_mode_frame_carries_truncated_text() {
    let frame = crsf::flight_mode_frame("Angle");
    assert_eq!(frame[0], 0xEC);
    assert_eq!(frame[2], 0x21, "CRSF flight-mode text frame type");
    assert_eq!(&frame[3..8], b"Angle");
    assert_eq!(frame[8], 0, "NUL-padded after the text");
    let long = crsf::flight_mode_frame("ACRO WITH A VERY LONG MODE NAME");
    assert_eq!(long.len(), 20, "address + length + type + 16 text bytes + crc");
    assert_eq!(&long[3..19], b"ACRO WITH A VERY");
}

#[test]
fn link_statistics_still_encodes_at_its_existing_shape() {
    // The bridge sends the sim's own numbers in the frame M3d already defined; this pins the payload size
    // the telemetry path relies on.
    assert_eq!(LINK_STATISTICS_PAYLOAD_SIZE, 10);
}
```

Add `serde_json` and `toml` to `crates/ofs-radio/Cargo.toml` `[dev-dependencies]`
(`serde_json.workspace = true`, `toml.workspace = true` — both are already workspace dependencies; check
`Cargo.toml` at the workspace root and add `workspace = true` entries only if the workspace defines them,
otherwise plain `"1"`/`"0.8"` versions).

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -p ofs-radio --test crsf_serial --locked`
Expected: compile errors — `ticks_to_stick`, `ticks_to_throttle`, `BatterySensor`, `battery_sensor_frame`,
`flight_mode_frame`, `telemetry_frame` do not exist.

- [ ] **Step 3: Implement**

In `crates/ofs-radio/src/crsf.rs`, after `ticks_to_us`:

```rust
/// CRSF ticks back to a stick position in [-1, 1]: the inverse of [`stick_ticks`], clamped — extended-servo
/// values beyond the calibrated 192..1792 range mean a stick pinned at its end, not an error.
pub fn ticks_to_stick(ticks: u16) -> f64 {
    ((f64::from(ticks) - 992.0) / 800.0).clamp(-1.0, 1.0)
}

/// CRSF ticks back to a throttle in [0, 1]: the inverse of [`throttle_ticks`], clamped the same way.
pub fn ticks_to_throttle(ticks: u16) -> f64 {
    ((f64::from(ticks) - 192.0) / 1600.0).clamp(0.0, 1.0)
}

/// The CRSF battery sensor's payload (frame type 0x08): what a handset's battery widget reads.
pub struct BatterySensor {
    pub voltage_mv: u16,
    pub current_ma: u16,
    pub capacity_used_mah: u32,
    pub remaining_pct: u8,
}

/// The frame type of the CRSF battery sensor payload.
pub const BATTERY_SENSOR_FRAME_TYPE: u8 = 0x08;
/// The frame type of the CRSF flight-mode text payload.
pub const FLIGHT_MODE_FRAME_TYPE: u8 = 0x21;
/// The frame type of the CRSF link statistics payload (M3d's encoder's own type, named for the bridge).
pub const LINK_STATISTICS_FRAME_TYPE: u8 = 0x14;
/// The CRSF address of the radio (handset) side: what telemetry frames to the handset are addressed to.
pub const RADIO_ADDRESS: u8 = 0xEC;
/// The flight-mode text field is 16 bytes, NUL-padded.
const FLIGHT_MODE_TEXT_LEN: usize = 16;

/// Builds a telemetry frame to the handset: address, length, type, payload, CRC8-DVB-S2.
pub fn telemetry_frame(frame_type: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.push(RADIO_ADDRESS);
    frame.push((payload.len() + 2) as u8); // type + payload + crc
    frame.push(frame_type);
    frame.extend_from_slice(payload);
    let crc = crc8_dvb_s2(&frame[2..]);
    frame.push(crc);
    frame
}

/// Encodes a battery sensor frame (type 0x08): voltage and current in 10 mV / 10 mA units, a u24 capacity,
/// one percent byte.
pub fn battery_sensor_frame(b: &BatterySensor) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&(b.voltage_mv / 10).to_le_bytes());
    payload.extend_from_slice(&(b.current_ma / 10).to_le_bytes());
    payload.push((b.capacity_used_mah & 0xFF) as u8);
    payload.push(((b.capacity_used_mah >> 8) & 0xFF) as u8);
    payload.push(((b.capacity_used_mah >> 16) & 0xFF) as u8);
    payload.push(b.remaining_pct);
    telemetry_frame(BATTERY_SENSOR_FRAME_TYPE, &payload)
}

/// Encodes a flight-mode text frame (type 0x21): up to 16 bytes of NUL-padded text.
///
/// # Panics
/// On empty text (the bridge skips the frame instead of sending a pointless one).
pub fn flight_mode_frame(text: &str) -> Vec<u8> {
    assert!(!text.is_empty(), "an empty flight-mode text is skipped by the caller, not sent");
    let bytes = text.as_bytes();
    let mut payload = vec![0u8; FLIGHT_MODE_TEXT_LEN];
    let n = bytes.len().min(FLIGHT_MODE_TEXT_LEN);
    payload[..n].copy_from_slice(&bytes[..n]);
    telemetry_frame(FLIGHT_MODE_FRAME_TYPE, &payload)
}
```

If the capture (Task 1) shows a different address byte for host→tx frames or different battery units, change
`RADIO_ADDRESS`/the encoders to match and say so in the commit message (the "capture wins" constraint).

- [ ] **Step 4: Run the crate's tests**

Run: `cargo test -p ofs-radio --locked`
Expected: PASS (existing crsf tests included). Clippy clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-radio crates/ofs-core Cargo.lock
git commit -m "feat(radio): CRSF tick conversion back from the wire; battery and flight-mode telemetry encoders

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 3: Protocol version 10 — the three RPCs, the messages, the event kinds

Spec §4. Generated code comes from the proto; nothing hand-written in `ofs-proto`.

**Files:**
- Modify: `proto/ofs/v1/sim.proto` (service block after line 44's `PlayInputs`/`StopInputs`; messages at the end
  of the file; event kinds after `EVENT_KIND_RECORDING_STOPPED = 19`)
- Modify: `crates/ofs-proto/src/lib.rs:12`, `python/ofs/client.py:26`
- Test: `crates/ofs-sim/tests/grpc.rs` (the existing handshake test's constants follow the crate constant —
  verify no literal `9` remains in the tests)

**Interfaces:**
- Produces: `pb::AttachSerialRadioRequest { port, baud, pass_through, record }`,
  `pb::AttachSerialRadioReply { recording_path }`, `pb::SerialPorts { ports }`,
  `pb::SerialPortInfo { name, description }`, `pb::EventKind::SerialRadioAttached/_Detached/_Lost` (20/21/22);
  `ofs_proto::PROTOCOL_VERSION == 10`.

- [ ] **Step 1: Edit the proto**

In the `service` block, after the `PlayInputs`/`StopInputs` pair (keep the comment style of the neighbours):

```proto
  // Attaches a real radio (an ELRS handset or TX module in its simulator mode) as the session's stick
  // source, through the simulated ELRS link unless pass_through. Real-time sessions only.
  rpc AttachSerialRadio(AttachSerialRadioRequest) returns (AttachSerialRadioReply);
  // Detaches it. A no-op when nothing is attached. The RC source goes silent (the receiver fails safe).
  rpc DetachSerialRadio(Empty) returns (Empty);
  // The serial ports this machine offers (USB serial devices first).
  rpc ListSerialPorts(Empty) returns (SerialPorts);
```

In the messages section, after the `InputsFile`/playback messages (end of file is fine — match the file's own
ordering):

```proto
message AttachSerialRadioRequest {
  string port = 1;        // from ListSerialPorts
  uint32 baud = 2;        // 0 = the captured default (docs/research/serial-radio.md)
  bool pass_through = 3;  // every packet delivers, bypassing the link model's loss (pure input use)
  bool record = 4;        // write the stick stream as a play_inputs CSV
}

message AttachSerialRadioReply {
  string recording_path = 1;  // empty unless record
}

message SerialPorts {
  repeated SerialPortInfo ports = 1;
}

message SerialPortInfo {
  string name = 1;          // what AttachSerialRadioRequest.port takes ("COM7", "/dev/ttyACM0")
  string description = 2;   // the OS's device description, when it has one
}
```

Event kinds (keeping the neighbouring comments' voice):

```proto
  EVENT_KIND_SERIAL_RADIO_ATTACHED = 20;  // the message names the port (and the recording, when asked for)
  EVENT_KIND_SERIAL_RADIO_DETACHED = 21;  // the message names the port (and the recording, when there is one)
  EVENT_KIND_SERIAL_RADIO_LOST = 22;      // the port died (cable pulled, radio off): the message says why
```

- [ ] **Step 2: Bump both version constants**

`crates/ofs-proto/src/lib.rs`: `pub const PROTOCOL_VERSION: u32 = 10;` (update any comment above it).
`python/ofs/client.py`: `PROTOCOL_VERSION = 10`.

- [ ] **Step 3: Build and run the proto-shaped tests**

Run: `cargo build -p ofs-sim --locked && cargo test -p ofs-sim --test grpc -p ofs-client --locked && python -m pytest python/tests/test_client.py -q`
Expected: PASS. If any test asserts a literal protocol `9`, update it to read `ofs_proto::PROTOCOL_VERSION`
(Rust) / `ofs.PROTOCOL_VERSION` (Python) instead of a literal — a literal here is exactly what bit us before.

- [ ] **Step 4: Commit**

```bash
git add proto crates/ofs-proto python/ofs/client.py crates/ofs-sim crates/ofs-client
git commit -m "feat(proto): protocol 10 - AttachSerialRadio/DetachSerialRadio/ListSerialPorts and the radio events

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 4: `ListSerialPorts` — the serialport dependency and the port list

Spec §4. The only new dependency in the workspace.

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`: `serialport = "4.7"`)
- Modify: `crates/ofs-sim/Cargo.toml` (`[dependencies]`: `serialport.workspace = true`)
- Modify: `crates/ofs-sim/src/radio_serial.rs` (create: the module with `list_ports`, the transport seam, and
  `DEFAULT_BAUD`/`TELEMETRY_PERIOD`)
- Modify: `crates/ofs-sim/src/lib.rs` (`mod radio_serial;`)
- Modify: `crates/ofs-sim/src/server.rs` (the `list_serial_ports` handler)
- Test: `crates/ofs-sim/tests/grpc.rs`

**Interfaces:**
- Produces (this task): `ofs_sim::radio_serial::{DEFAULT_BAUD: u32, TELEMETRY_PERIOD: Duration, PortInfo, list_ports() -> Vec<PortInfo>}`;
  the gRPC `ListSerialPorts` handler returning them.
- Consumes: `pb::SerialPorts`/`pb::SerialPortInfo` (Task 3).

- [ ] **Step 1: Write the failing test**

Append to `crates/ofs-sim/tests/grpc.rs` (its helpers — a started service on a free port — exist; mirror the
nearest existing handler test's setup):

```rust
#[tokio::test]
async fn list_serial_ports_reports_the_machines_ports_or_an_empty_list() {
    let (svc, _guard) = service().await;
    let reply = svc.list_serial_ports(Request::new(pb::Empty {})).await.unwrap().into_inner();
    // No port is guaranteed on a CI machine: the shape is the contract. Whatever is listed carries both
    // fields, and names are unique.
    let names: Vec<&str> = reply.ports.iter().map(|p| p.name.as_str()).collect();
    let unique = names.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique.len(), names.len(), "duplicate port names: {names:?}");
    for p in &reply.ports {
        assert!(!p.name.is_empty(), "a port entry has a name: {p:?}");
    }
}
```

- [ ] **Step 2: Run it to see the failure**

Run: `cargo test -p ofs-sim --test grpc --locked list_serial_ports`
Expected: compile error — no `list_serial_ports` on the service.

- [ ] **Step 3: Implement the module and the handler**

Create `crates/ofs-sim/src/radio_serial.rs` with the module doc comment from the spec's §3 (name the spec), then:

```rust
/// The captured link baud (`docs/research/serial-radio.md`); a request's `baud: 0` means this.
pub const DEFAULT_BAUD: u32 = 420_000;
/// The telemetry cadence to the handset (`docs/research/serial-radio.md`, the bridge's ruling).
pub const TELEMETRY_PERIOD: Duration = Duration::from_millis(100);
/// A blocking read never waits longer than this, so the thread checks its stop flag at least this often.
pub(crate) const READ_TIMEOUT: Duration = Duration::from_millis(20);

/// Where serial bytes come from and go. The real port is `serialport`; tests inject a duplex; the Linux
/// end-to-end tests use a pty. Read blocks at most `READ_TIMEOUT` (0 bytes on timeout); an error means the
/// port is gone (unplug, radio off).
pub trait SerialTransport: Send {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize>;
}

/// A port as `ListSerialPorts` reports it.
pub struct PortInfo {
    pub name: String,
    pub description: String,
}

/// The machine's serial ports: USB serial devices (a handset is one) and anything the OS reports as unknown;
/// Bluetooth and PCI ports are not radio candidates.
pub fn list_ports() -> Vec<PortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(info) => Some(PortInfo {
                description: info.product.clone().unwrap_or_else(|| "USB serial device".into()),
                name: p.port_name,
            }),
            serialport::SerialPortType::Unknown => {
                Some(PortInfo { description: String::new(), name: p.port_name })
            }
            _ => None,
        })
        .collect()
}

/// Opens the real port. The only place `serialport` is called.
pub(crate) fn open_port(port: &str, baud: u32) -> std::io::Result<Box<dyn SerialTransport>> {
    serialport::new(port, baud).timeout(READ_TIMEOUT).open().map(|p| Box::new(p) as Box<dyn SerialTransport>)
}

impl SerialTransport for Box<dyn serialport::SerialPort> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(self.as_mut(), buf)
    }

    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(self.as_mut(), bytes)
    }
}
```

with the file's `use` block: `std::io` traits as needed, `std::time::{Duration, Instant}`,
`std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}}`.

`crates/ofs-sim/src/lib.rs`: add `pub mod radio_serial;` beside the other modules.

The handler, in `crates/ofs-sim/src/server.rs`'s service impl (beside `play_inputs`):

```rust
    async fn list_serial_ports(&self, _req: Request<pb::Empty>) -> Result<Response<pb::SerialPorts>, Status> {
        let ports = crate::radio_serial::list_ports()
            .into_iter()
            .map(|p| pb::SerialPortInfo { name: p.name, description: p.description })
            .collect();
        Ok(Response::new(pb::SerialPorts { ports }))
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS, clean. (`serialport` pulls build deps on Linux: `libudev-dev` — CI's Linux job already has it
via the existing rust toolchain steps; if the CI job lacks it, add `libudev-dev` to that job's apt step and say
so in the commit message.)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/ofs-sim
git commit -m "feat(sim): ListSerialPorts - the serialport dependency and the port list (M5, spec 4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 5: The bridge's control plane — attach, detach, exclusivity, lifecycle

Spec §3 (decisions 1, 3, 9). The handlers, the flags, the thread's lifecycle — with an in-memory transport that
sends nothing yet (data plane is Task 6). Unit tests live in the module (`#[cfg(test)]`) because they need
crate-internal session helpers.

**Files:**
- Modify: `crates/ofs-sim/src/radio_serial.rs` (the thread, `spawn_bridge_with`, the handle, the recorder type
  declared but row-writing lands in Task 7)
- Modify: `crates/ofs-sim/src/session.rs` (`Shared.serial_radio: AtomicBool`; `Session.serial_radio:
  Option<SerialRadioHandle>` declared **last** — fields drop in declaration order, and the handle must outlive
  nothing)
- Modify: `crates/ofs-sim/src/server.rs` (the `attach_serial_radio`/`detach_serial_radio` handlers; refusals in
  the `pilot`, `play_inputs`, and `set_sticks` handlers)
- Test: `crates/ofs-sim/src/radio_serial.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `radio_serial::AttachRequest { port: String, baud: u32, pass_through: bool, record: bool }`;
  `radio_serial::spawn_bridge_with(shared: Arc<Shared>, logs_dir: PathBuf, quad_stem: String, session_id: u64,
  request: AttachRequest, transport: Box<dyn SerialTransport>) -> Result<(SerialRadioHandle,
  Option<PathBuf>), String>` (public: the pty end to end and tests both inject); `radio_serial::spawn_bridge(..)`
  = `open_port` + `spawn_bridge_with` (what the handler calls); `SerialRadioHandle { port: String, .. }` with
  `stop_and_wait(self)`; `Shared::serial_radio` (AtomicBool, true while a bridge runs); the gRPC handlers.
  `Session::serial_radio` is `pub(crate)` — set only by the attach handler, taken only by detach/load.

- [ ] **Step 1: Write the failing tests**

Append to `radio_serial.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::open_loop_session;
    use crate::session::RunMode;

    /// An in-memory peer: the test writes what the "handset" sends, and reads what the bridge answers.
    /// Reads time out at READ_TIMEOUT so the loop keeps checking its stop flag.
    pub(crate) struct Duplex {
        inbound: mpsc::Receiver<Vec<u8>>,
        outbound: mpsc::Sender<Vec<u8>>,
    }

    impl SerialTransport for Duplex {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.inbound.recv_timeout(READ_TIMEOUT) {
                Ok(bytes) => {
                    let n = bytes.len().min(buf.len());
                    buf[..n].copy_from_slice(&bytes[..n]);
                    Ok(n)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(0),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::other("the peer closed the port")),
            }
        }

        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.outbound.send(bytes.to_vec()).map_err(|_| std::io::Error::other("the peer is gone"))?;
            Ok(bytes.len())
        }
    }

    /// (transport input, transport output): the test holds the handset side.
    pub(crate) fn duplex() -> (Duplex, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (in_tx, in_rx) = mpsc::channel();
        let (out_tx, out_rx) = mpsc::channel();
        (Duplex { inbound: in_rx, outbound: out_tx }, in_tx, out_rx)
    }

    /// A shared slot holding one open-loop session, ready for a bridge.
    pub(crate) fn rigged(mode: RunMode) -> (Arc<Shared>, u64) {
        let shared = Shared::new();
        let session = open_loop_session(mode);
        let id = session.id;
        *shared.lock() = Some(session);
        (shared, id)
    }

    fn request() -> AttachRequest {
        AttachRequest { port: "TEST".into(), baud: 0, pass_through: false, record: false }
    }

    #[test]
    fn attach_refuses_a_lockstep_session_and_a_double_attach() {
        let (shared, id) = rigged(RunMode::Lockstep);
        let (transport, _in, _out) = duplex();
        let err = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport))
            .err().expect("lockstep refuses a live radio");
        assert!(err.contains("lockstep"), "{err}");
        // Now real-time, attached once:
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, _in, _out) = duplex();
        let (handle, recording) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        assert!(recording.is_none());
        let (transport2, _in2, _out2) = duplex();
        let err = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport2))
            .err().expect("a second attach is refused");
        assert!(err.contains("already attached"), "{err}");
        handle.stop_and_wait();
        assert!(!shared.serial_radio.load(Ordering::Acquire), "the flag clears when the bridge stops");
    }

    #[test]
    fn an_open_failure_is_an_error_naming_the_port_and_clears_the_flag() {
        let (shared, _id) = rigged(RunMode::Realtime);
        let logs = tmp_logs();
        let err = spawn_bridge(shared.clone(), logs, "quad".into(), 1, AttachRequest {
            port: "/ofs/no-such-port-5f".into(), baud: 0, pass_through: false, record: false,
        }).err().expect("a nonexistent port fails");
        assert!(err.contains("/ofs/no-such-port-5f"), "{err}");
        assert!(!shared.serial_radio.load(Ordering::Acquire), "a failed attach holds nothing");
    }

    #[test]
    fn detach_is_a_noop_without_a_radio_and_stops_the_thread_with_one() {
        let (shared, id) = rigged(RunMode::Realtime);
        // Detach with nothing attached: the handler-level no-op is tested through the flag it would clear.
        assert!(!shared.serial_radio.load(Ordering::Acquire));
        let (transport, _in, _out) = duplex();
        let (handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        handle.stop_and_wait(); // what the detach handler does after publishing its event
        // A stopped bridge's thread is gone: a fresh attach succeeds.
        let (transport, _in, _out) = duplex();
        let (handle2, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        handle2.stop_and_wait();
    }

    #[test]
    fn a_peer_that_closes_the_port_ends_the_bridge_as_lost() {
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, in_tx, _out) = duplex();
        let (handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        drop(in_tx); // the peer "unplugged": reads fail
        handle.stop_and_wait();
        // The thread ended by itself; the handle's stop flag is irrelevant. The LOST event is published by
        // the loop (Task 6 asserts the event); here we pin that the flag clears so a re-attach can happen.
        assert!(!shared.serial_radio.load(Ordering::Acquire));
    }
}
```

And the handler-level half of the exclusivity matrix, in `crates/ofs-sim/tests/grpc.rs` (its `service()`
helper and its existing pilot test hold the arrangement to mirror — reuse however that test connects its
pilot stream):

```rust
#[tokio::test]
async fn attach_serial_radio_is_refused_while_a_pilot_holds_the_sticks() {
    let (svc, _guard) = service().await;
    // Load a real-time session the way the file's other tests do, then connect a pilot stream the way
    // the file's existing pilot test does (mirror its setup exactly).
    load_realtime(&svc).await;
    let (pilot_tx, pilot_rx) = tokio::sync::mpsc::channel(8);
    pilot_tx.send(pb::PilotInput { sticks: Some(pb::Sticks::default()), state_rate_hz: 60 }).await.unwrap();
    let mut pilot_stream = svc
        .pilot(Request::new(tonic::Streaming::from(inner(pilot_rx))))
        .await
        .expect("the pilot connects");
    let err = svc
        .attach_serial_radio(Request::new(pb::AttachSerialRadioRequest {
            port: "TEST".into(), baud: 0, pass_through: false, record: false,
        }))
        .await
        .err()
        .expect("attach is refused while a pilot holds the sticks");
    assert_eq!(err.code(), Code::FailedPrecondition, "{err}");
    assert!(err.message().contains("pilot"), "{err}");
    drop(pilot_stream);
    drop(pilot_tx);
}
```

(The exact streaming-request construction follows the file's existing pilot test — `tonic::Streaming::from` /
`Request::new` on a receiver, whichever that test uses; `load_realtime` is the file's existing load helper or a
two-line addition following its lockstep helper. If the file's pilot test drives the stream differently, mirror
it — the assertion is the point, not the plumbing.)

with helpers in the test module:

```rust
    fn tmp_logs() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ofs-radio-{}-{}", std::process::id(), std::time::Instant::now().elapsed().subsec_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
```

(`open_loop_session` exists at `crates/ofs-sim/src/session.rs:445` and builds a paused real-time or lockstep
session with the shipped quad — check its exact signature when writing; `Shared`'s `lock()` puts the session in
the slot via `*shared.lock() = Some(session)`.)

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -p ofs-sim --lib radio_serial --locked`
Expected: compile errors — `spawn_bridge_with`, `AttachRequest`, `SerialRadioHandle`, `Shared::serial_radio` do
not exist.

- [ ] **Step 3: Implement the control plane**

In `session.rs`'s `Shared`: add `pub serial_radio: AtomicBool` (initialized `AtomicBool::new(false)` next to
`pilot`), with the doc comment `/// A serial radio bridge is attached (the one-stick-owner rule's third member).`

In `session.rs`'s `Session`: add `pub(crate) serial_radio: Option<crate::radio_serial::SerialRadioHandle>`
**as the last field** (the Blackbox lesson: fields drop in declaration order; the handle only sets a stop flag on
drop), initialized `None` in `Session::new`.

In `radio_serial.rs` (still no data plane — the loop only stops):

```rust
/// What AttachSerialRadio asked for. `baud: 0` means [`DEFAULT_BAUD`].
#[derive(Debug, Clone)]
pub struct AttachRequest {
    pub port: String,
    pub baud: u32,
    pub pass_through: bool,
    pub record: bool,
}

/// A running bridge. Dropping it tells the thread to stop (it checks before it next takes the slot lock, and
/// its reads never block past `READ_TIMEOUT`); `stop_and_wait` also waits for the exit so a detach's caller
/// knows the port is released and a radio CSV is flushed.
pub struct SerialRadioHandle {
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
    /// The port this bridge holds, for the detach/lost events' messages.
    pub(crate) port: String,
    /// Set when the loop ended because the port died, with the OS error: the LOST event's reason.
    pub(crate) lost: Arc<std::sync::Mutex<Option<String>>>,
}

impl SerialRadioHandle {
    pub(crate) fn stop_and_wait(mut self) {
        self.stop.store(true, Ordering::Release);
        while matches!(self.done.recv_timeout(READ_TIMEOUT), Err(RecvTimeoutError::Timeout)) {
            self.stop.store(true, Ordering::Release); // one read timeout is enough in practice; be insistent
        }
    }
}

impl Drop for SerialRadioHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// Opens the real port and spawns the bridge: what the attach handler calls.
pub(crate) fn spawn_bridge(
    shared: Arc<Shared>,
    logs_dir: PathBuf,
    quad_stem: String,
    session_id: u64,
    request: AttachRequest,
) -> Result<(SerialRadioHandle, Option<PathBuf>), String> {
    let baud = if request.baud == 0 { DEFAULT_BAUD } else { request.baud };
    let transport = open_port(&request.port, baud).map_err(|e| format!("could not open {}: {e}", request.port))?;
    spawn_bridge_with(shared, logs_dir, quad_stem, session_id, request, transport)
}

/// Spawns the bridge on a caller-supplied transport (tests and the pty end to end inject here).
pub fn spawn_bridge_with(
    shared: Arc<Shared>,
    logs_dir: PathBuf,
    quad_stem: String,
    session_id: u64,
    request: AttachRequest,
    transport: Box<dyn SerialTransport>,
) -> Result<(SerialRadioHandle, Option<PathBuf>), String> {
    if shared.serial_radio.swap(true, Ordering::AcqRel) {
        return Err("a serial radio is already attached (DetachSerialRadio first)".into());
    }
    let recorder = match request.record {
        true => Some(StickRecorder::create(&logs_dir, &quad_stem)?),
        false => None,
    };
    let recording_path = recorder.as_ref().map(|r| r.path.clone());
    let stop = Arc::new(AtomicBool::new(false));
    let lost = Arc::new(std::sync::Mutex::new(None));
    let (done_tx, done_rx) = mpsc::channel();
    let thread_stop = stop.clone();
    let thread_lost = lost.clone();
    let port = request.port.clone();
    let spawned = std::thread::Builder::new()
        .name("ofs-serial-radio".into())
        .spawn(move || bridge_loop(shared, session_id, request, transport, recorder, thread_stop, thread_lost, done_tx));
    match spawned {
        Ok(_) => Ok((SerialRadioHandle { stop, done: done_rx, port, lost }, recording_path)),
        Err(e) => {
            shared.serial_radio.store(false, Ordering::Release);
            Err(format!("could not start the serial-radio thread: {e}"))
        }
    }
}

#[allow(unused_variables)] // the data plane (Task 6) fills this in
fn bridge_loop(
    shared: Arc<Shared>,
    session_id: u64,
    request: AttachRequest,
    mut transport: Box<dyn SerialTransport>,
    mut recorder: Option<StickRecorder>,
    stop: Arc<AtomicBool>,
    lost: Arc<std::sync::Mutex<Option<String>>>,
    done: Sender<()>,
) {
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        // Task 6: read bytes, decode frames, apply sticks, answer telemetry.
        std::thread::sleep(READ_TIMEOUT);
    }
    *lost.lock().unwrap_or_else(|e| e.into_inner()) = None; // Task 6 sets the real reason
    if let Some(recorder) = recorder {
        recorder.finish();
    }
    shared.serial_radio.store(false, Ordering::Release);
    let _ = done.send(());
}
```

and the recorder (its `row` is Task 7; declare it now with the file open and a header — the header is this
task's deliverable so a detach produces a parseable file even with no frames):

```rust
/// The stick stream as a `play_inputs` CSV (M4c's format, header first): one row per channel frame applied.
struct StickRecorder {
    file: std::fs::File,
    path: PathBuf,
}

impl StickRecorder {
    fn create(logs_dir: &Path, quad_stem: &str) -> Result<Self, String> {
        std::fs::create_dir_all(logs_dir).map_err(|e| format!("could not create {}: {e}", logs_dir.display()))?;
        for n in 1.. {
            let name = if n == 1 { format!("{quad_stem}-radio.csv") } else { format!("{quad_stem}-radio-{n}.csv") };
            let path = logs_dir.join(name);
            match std::fs::OpenOptions::new().create_new(true).append(true).open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    writeln!(file, "t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4")
                        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                    return Ok(Self { file, path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("could not create {}: {e}", path.display())),
            }
        }
        unreachable!("the loop only exits on success or error")
    }

    fn finish(self) {
        // Flushed by the file's drop; nothing else to do. Detach waits for the thread before answering, so
        // the CSV is complete when the RPC returns.
    }
}
```

The handlers in `server.rs` (beside `play_inputs`; `error`/`invalid_state`/`loaded` already exist there):

```rust
    async fn attach_serial_radio(
        &self,
        req: Request<pb::AttachSerialRadioRequest>,
    ) -> Result<Response<pb::AttachSerialRadioReply>, Status> {
        let cfg = req.into_inner();
        if cfg.port.is_empty() {
            return Err(error("invalid_argument", Code::InvalidArgument, "name a port (ListSerialPorts lists them)"));
        }
        let shared = self.shared.clone();
        let logs_dir = self.logs_dir();
        let outcome = self
            .blocking(move |shared0, slot| {
                let s = loaded(slot)?;
                if shared0.pilot.load(Ordering::Acquire) {
                    return Err(invalid_state("a pilot is connected: the radio would fight the pilot's sticks"));
                }
                if let Some(playing) = s.vehicle.inputs_playing() {
                    return Err(invalid_state(&format!("{playing} is playing scripted inputs; StopInputs before attaching a radio")));
                }
                if s.mode == RunMode::Lockstep {
                    return Err(error(
                        "lockstep_radio",
                        Code::FailedPrecondition,
                        "a live serial radio cannot fly a lockstep session (a live port is not reproducible); \
                         load in real time, or record the flight and replay it in lockstep",
                    ));
                }
                s.vehicle.set_radio_passthrough(cfg.pass_through); // Task 6 adds the vehicle method
                let session_id = s.id;
                let quad_stem = s.quad_stem.clone();
                match crate::radio_serial::spawn_bridge(shared.clone(), logs_dir, quad_stem, session_id, cfg.into()) {
                    Ok((handle, recording)) => {
                        s.serial_radio = Some(handle);
                        Ok(recording)
                    }
                    Err(message) => Err(error("serial", Code::InvalidArgument, message)),
                }
            })
            .await?;
        // Out of the closure (its lock is released): announce what happened. The event must name the port,
        // so re-derive it from the reply's session — the handler knows the port from cfg.
        let message = match &outcome {
            Some(path) => format!("{} (recording to {})", cfg.port, path),
            None => cfg.port.clone(),
        };
        if let Ok(state) = self.state(pb::Empty {}).await {
            self.publish_event(state.into_inner().time_s, pb::EventKind::SerialRadioAttached, message);
        }
        Ok(Response::new(pb::AttachSerialRadioReply {
            recording_path: outcome.map(|p| p.display().to_string()).unwrap_or_default(),
        }))
    }
```

**Stop — that draft leaks the port through `cfg` after the move and invents `publish_event`/`self.state`. Do it
the way the pilot path does instead:** the closure itself publishes the event while it still holds the session
(`shared0.publish([...])` — `Shared::publish` exists and `event()` is `crate::session::event`), and the closure
returns the reply struct, not the path. Write the handler as:

```rust
    async fn attach_serial_radio(
        &self,
        req: Request<pb::AttachSerialRadioRequest>,
    ) -> Result<Response<pb::AttachSerialRadioReply>, Status> {
        let cfg = req.into_inner();
        if cfg.port.is_empty() {
            return Err(error("invalid_argument", Code::InvalidArgument, "name a port (ListSerialPorts lists them)"));
        }
        let shared = self.shared.clone();
        let logs_dir = self.logs_dir();
        let reply = self
            .blocking(move |shared0, slot| {
                let s = loaded(slot)?;
                if shared0.pilot.load(Ordering::Acquire) {
                    return Err(invalid_state("a pilot is connected: the radio would fight the pilot's sticks"));
                }
                if let Some(playing) = s.vehicle.inputs_playing() {
                    return Err(invalid_state(&format!("{playing} is playing scripted inputs; StopInputs before attaching a radio")));
                }
                if s.mode == RunMode::Lockstep {
                    return Err(error(
                        "lockstep_radio",
                        Code::FailedPrecondition,
                        "a live serial radio cannot fly a lockstep session (a live port is not reproducible); \
                         load in real time, or record the flight and replay it in lockstep",
                    ));
                }
                s.vehicle.set_radio_passthrough(cfg.pass_through);
                let session_id = s.id;
                let quad_stem = s.quad_stem.clone();
                let request = crate::radio_serial::AttachRequest {
                    port: cfg.port.clone(),
                    baud: cfg.baud,
                    pass_through: cfg.pass_through,
                    record: cfg.record,
                };
                match crate::radio_serial::spawn_bridge(shared.clone(), logs_dir, quad_stem, session_id, request) {
                    Ok((handle, recording)) => {
                        s.serial_radio = Some(handle);
                        let baud = if cfg.baud == 0 { crate::radio_serial::DEFAULT_BAUD } else { cfg.baud };
                        let message = match &recording {
                            Some(path) => format!("{} at {} baud (recording to {})", cfg.port, baud, path.display()),
                            None => format!("{} at {} baud", cfg.port, baud),
                        };
                        shared0.publish([crate::session::event(
                            s.vehicle.time_s(),
                            pb::EventKind::SerialRadioAttached,
                            message,
                        )]);
                        Ok(pb::AttachSerialRadioReply {
                            recording_path: recording.map(|p| p.display().to_string()).unwrap_or_default(),
                        })
                    }
                    Err(message) => Err(error("serial", Code::InvalidArgument, message)),
                }
            })
            .await?;
        Ok(Response::new(reply))
    }

    async fn detach_serial_radio(&self, _req: Request<pb::Empty>) -> Result<Response<pb::Empty>, Status> {
        let shared = self.shared.clone();
        // The handle comes out of the closure; stop_and_wait runs with the lock released (the dying thread
        // takes that lock on its way out).
        struct Detached(Option<crate::radio_serial::SerialRadioHandle>, Option<String>);
        let Detached(handle, recording) = self
            .blocking(move |shared0, slot| {
                let s = loaded(slot)?;
                match s.serial_radio.take() {
                    Some(handle) => {
                        let port = handle.port.clone();
                        let recording = None; // Task 7 sets this from the bridge's recorder path
                        s.vehicle.set_transmitter(false);
                        // A bridge that already died on a dead port published LOST; do not also say DETACHED.
                        let already_lost = handle.lost.lock().unwrap_or_else(|e| e.into_inner()).is_some();
                        if !already_lost {
                            shared0.publish([crate::session::event(
                                s.vehicle.time_s(),
                                pb::EventKind::SerialRadioDetached,
                                port.clone(),
                            )]);
                        }
                        Detached(Some(handle), recording)
                    }
                    None => Detached(None, None), // a no-op when nothing is attached
                }
            })
            .await?;
        if let Some(handle) = handle {
            handle.stop_and_wait();
        }
        Ok(Response::new(pb::Empty {}))
    }
```

(`run_mode`/`RunMode` is imported in server.rs already for Load — verify; `self.shared` and `self.logs_dir()`
are the service's existing fields/methods — verify the exact names when writing; `AttachRequest` needs
`impl From<pb::AttachSerialRadioRequest> for AttachRequest` or the explicit struct literal shown.)

The three refusals, in the existing handlers:

- `pilot` (in `crates/ofs-sim/src/streams.rs`'s `spawn_blocking` closure, next to the `inputs_playing` check):
  `if s.serial_radio ...` — the closure has `s = shared.clone()` (an `Arc<Shared>`), so:
  `if s.serial_radio.load(Ordering::Acquire) { return Err(error("invalid_state", Code::FailedPrecondition, "a serial radio is attached: detach it before piloting")); }` — check `error`'s import there (the file already
  builds Status values).
- `play_inputs` (server.rs, next to the pilot check):
  `if shared.serial_radio.load(Ordering::Acquire) { return Err(invalid_state("a serial radio is attached: scripted inputs would fight the radio's sticks")); }`
- `set_sticks` (server.rs, beside the playback check — `InvalidArgument`, per the M4c ruling's symmetry):
  `if shared.serial_radio.load(Ordering::Acquire) { return Err(error("invalid_argument", Code::InvalidArgument, "a serial radio is attached: SetSticks would be overwritten (DetachSerialRadio first)")); }`

Load replaces the session: in the `Load` handler, after the old session is dropped/replaced, the handle's Drop
already told the thread to stop — add `shared0.serial_radio.store(false, Ordering::Release);` to the same
closure so a fresh attach is possible at once (the dying thread's exit also stores false; both are correct —
the flag is `false` iff no bridge may apply sticks, and the id check in the loop is the real guard).

`crates/ofs-sim/src/vehicle.rs`: add the passthrough setter the handler calls (Task 6 reads it), and the
crate-visible RC reader the bridge tests use (the bus is the truth of what the last stick source wrote; `State`
deliberately carries no sticks — clients see the world, not the wire):

```rust
    /// The serial-radio pass-through flag (M5): every packet delivers while this is set.
    pub fn set_radio_passthrough(&mut self, on: bool) {
        let s = self.h.radio_passthrough;
        self.scheduler.bus_mut().set(s, if on { 1.0 } else { 0.0 });
    }

    /// The RC channels the last stick source wrote (M5): roll, pitch, yaw, throttle, aux1-4.
    pub(crate) fn rc_channels(&self) -> [f64; 8] {
        let bus = self.scheduler.bus();
        let mut out = [
            bus.get(self.h.roll),
            bus.get(self.h.pitch),
            bus.get(self.h.yaw),
            bus.get(self.h.throttle),
        ];
        for (i, sig) in self.h.aux.iter().enumerate() {
            out[4 + i] = bus.get(*sig);
        }
        out
    }
```

with `radio_passthrough: Signal<f64>` in the handles struct, registered as
`bus.signal(&names::RADIO_PASSTHROUGH)` in `build` (the same pattern as `tx_enabled`), and the name in
`crates/ofs-core/src/names.rs` beside the other radio names:

```rust
/// The serial-radio bridge's pass-through flag (M5): 1 while a radio is attached in pass-through mode.
pub const RADIO_PASSTHROUGH: &str = "radio.passthrough";
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --locked` (all of it — the grpc tests prove the new handlers compile and the
existing exclusivity tests still pass) and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
Expected: PASS, clean. Task 5's tests pass (control plane); the loop is still a stub, so the
`a_peer_that_closes_the_port` test passes via `stop_and_wait` only.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-core crates/ofs-sim
git commit -m "feat(sim): the serial-radio bridge's control plane - attach/detach, one-stick-owner, lifecycle

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 6: The bridge's data plane — channels to the bus, telemetry home, LOST

Spec §3 (decisions 2, 4, 9) and §5's telemetry. The loop becomes real; pass-through lands in `ElrsLink`.

**Files:**
- Modify: `crates/ofs-sim/src/radio_serial.rs` (`bridge_loop`, `sticks_from_channels`, `telemetry_frames`,
  the LOST event)
- Modify: `crates/ofs-radio/src/elrs.rs` (the pass-through read in `step`)
- Test: `crates/ofs-sim/src/radio_serial.rs` (`#[cfg(test)] mod tests` grows), `crates/ofs-radio/tests/elrs.rs`

**Interfaces:**
- Consumes: `crsf::ticks_to_stick`/`ticks_to_throttle`/`battery_sensor_frame`/`flight_mode_frame`/
  `telemetry_frame` (Task 2); the control plane (Task 5); `Vehicle::state()` (existing snapshot: `radio`,
  `battery_voltage_v`, `battery_current_a`, `battery_soc`, `battery_consumed_mah`, `fc_telemetry` — verify the
  exact field names on `VehicleState` when writing; `state().fc_telemetry.flight_mode` is empty in open loop).
- Produces: the behaviours themselves. `vehicle.state()` needs one addition if the sticks are not already
  exposed: the test reads applied sticks through `session.state_msg().sticks` (existing) — no vehicle change.

- [ ] **Step 1: Write the failing tests**

Append to `radio_serial.rs`'s test module (helpers from Task 5):

```rust
    use ofs_radio::crsf;
    use ofs_radio::names;

    /// One RC_CHANNELS frame (CRSF): address 0xC8 (to the FC), type 0x16, 16 channels of 11 bits.
    fn rc_frame(ticks: [u16; 16]) -> Vec<u8> {
        crsf::rc_channels_frame(&ticks)
    }

    fn center() -> [u16; 16] {
        let mut ch = [992u16; 16];
        ch[3] = crsf::throttle_ticks(0.0);
        ch
    }

    #[test]
    fn channels_reach_the_bus_through_the_same_set_sticks_path() {
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, in_tx, _out) = duplex();
        let (_handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        let mut ticks = center();
        ticks[0] = crsf::stick_ticks(-0.5);
        ticks[3] = crsf::throttle_ticks(0.7);
        in_tx.send(rc_frame(ticks)).unwrap();
        // The bridge applies it within a read timeout or two; poll the bus (Vehicle::rc_channels, Task 5).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let rc = loop {
            let rc = shared.lock().as_ref().expect("session gone").vehicle.rc_channels();
            if (rc[3] - 0.7).abs() < 1e-6 && (rc[0] + 0.5).abs() < 1e-6 {
                break rc;
            }
            assert!(std::time::Instant::now() < deadline, "the channels never reached the bus: {rc:?}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(rc[1], 0.0);
    }

    #[test]
    fn out_of_range_channels_clamp_and_channels_beyond_eight_are_dropped() {
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, in_tx, _out) = duplex();
        let (_handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        let mut ticks = center();
        ticks[0] = 1811; // extended-servo end: clamps to 1.0, never NaN
        ticks[8] = 42;   // channel 9: dropped
        in_tx.send(rc_frame(ticks)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let rc = shared.lock().as_ref().unwrap().vehicle.rc_channels();
            if rc[0] == 1.0 {
                assert!(rc[0].is_finite());
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the clamped channel never arrived: {rc:?}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn garbage_bytes_never_stall_or_grow_the_bridge() {
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, in_tx, _out) = duplex();
        let (_handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        // 4 kB of noise, then one good frame: the decoder resyncs and the frame applies.
        in_tx.send(vec![0xA5u8; 4096]).unwrap();
        let mut ticks = center();
        ticks[1] = crsf::stick_ticks(0.25);
        in_tx.send(rc_frame(ticks)).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        loop {
            let rc = shared.lock().as_ref().unwrap().vehicle.rc_channels();
            if (rc[1] - 0.25).abs() < 1e-6 {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "a frame after noise never applied: {rc:?}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn unplugging_the_port_publishes_lost_and_leaves_the_transmitter_off() {
        let (shared, id) = rigged(RunMode::Realtime);
        let mut events = shared.events.subscribe();
        let (transport, in_tx, _out) = duplex();
        let (handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        in_tx.send(rc_frame(center())).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200)); // let one frame apply
        drop(in_tx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match events.try_recv() {
                Ok(e) if e.kind == pb::EventKind::SerialRadioLost as i32 => {
                    assert!(e.message.contains("TEST"), "the lost event names the port: {}", e.message);
                    break;
                }
                _ if std::time::Instant::now() > deadline => panic!("no SERIAL_RADIO_LOST event arrived"),
                _ => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        let slot = shared.lock();
        assert!(!shared.serial_radio.load(Ordering::Acquire), "a lost bridge holds nothing");
        drop(slot);
        handle.stop_and_wait(); // already stopped; returns at once
    }

    #[test]
    fn the_bridge_answers_telemetry_from_the_vehicle_state() {
        let (shared, id) = rigged(RunMode::Realtime);
        let (transport, in_tx, out_rx) = duplex();
        let (_handle, _) = spawn_bridge_with(shared.clone(), tmp_logs(), "quad".into(), id, request(), Box::new(transport)).unwrap();
        in_tx.send(rc_frame(center())).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut decoder = Decoder::default();
        loop {
            match out_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(bytes) => {
                    for frame in decoder.push(&bytes) {
                        if let Frame::LinkStatistics(ls) = frame {
                            // The sim's own numbers: the rigged open-loop session sits on the pad, LQ 100.
                            assert_eq!(ls.uplink_lq, 100, "the handset sees the simulated link's LQ: {ls:?}");
                            return; // the battery/flight-mode frames are the SITL suite's (Task 11)
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(e) => panic!("the telemetry channel died: {e}"),
            }
            assert!(std::time::Instant::now() < deadline, "no LINK_STATISTICS reply within 3 s");
        }
    }
```

(The telemetry frame's address/type per Task 2's encoders; if the capture says the bridge should answer on a
different rhythm, `TELEMETRY_PERIOD` changes with it.)

In `crates/ofs-radio/tests/elrs.rs`:

```rust
/// M5: pass-through delivers every packet whatever the geometry — the handset flies pure input, the link
/// model's loss draws still happen in their fixed positions (a zero-flag run's downlink history is
/// unchanged), and LQ reads 100.
#[test]
fn pass_through_delivers_every_packet_and_does_not_move_the_draws() {
    let lq_history = |passthrough: f64| -> Vec<f64> {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        let (mut s, _uart, _telemetry_in, _fc) = rig(p, 21);
        set_pos(&mut s, DVec3::new(90_000.0, 0.0, -1.7)); // far beyond the sensitivity edge
        set(&mut s, names::RADIO_PASSTHROUGH, passthrough);
        set(&mut s, names::RADIO_TX_ENABLED, 1.0);
        (0..100).map(|_| { packets(&mut s, 1); get(&s, names::RADIO_DOWNLINK_LQ) }).collect()
    };
    let with = lq_history(1.0);
    assert!(with.iter().all(|&lq| lq == 100.0), "pass-through delivers: {with:?}");
    let without = lq_history(0.0);
    assert!(without.iter().all(|&lq| lq == 0.0), "90 km is dead without pass-through: {without:?}");
    // The draw-order discipline: the *downlink* history of a close-range rig is identical with and without
    // the flag, because the flag changes only which results are used.
    let close = |passthrough: f64| -> Vec<f64> {
        let mut p = LinkParams::ideal(500);
        p.fading = true;
        let (mut s, _uart, _telemetry_in, _fc) = rig(p, 22);
        set_pos(&mut s, DVec3::new(20.0, 0.0, -1.7));
        set(&mut s, names::RADIO_PASSTHROUGH, passthrough);
        set(&mut s, names::RADIO_TX_ENABLED, 1.0);
        (0..100).map(|_| { packets(&mut s, 1); get(&s, names::RADIO_DOWNLINK_LQ) }).collect()
    };
    assert_eq!(close(0.0), close(1.0), "the flag never moves the model's own draws");
}
```

(The `rig`/`set`/`get`/`packets` helpers and the DVec3 import exist in that test file; the 90 km dead-link
distance is the M3e test's own finding — if 90 km is not dead for this config, pick the distance from
`the_models_own_stream_ignores_the_fault_fraction`'s edge search, above the edge.)

- [ ] **Step 2: Run them to see the failures**

Run: `cargo test -p ofs-sim --lib radio_serial --locked` and `cargo test -p ofs-radio --test elrs --locked -- pass_through`
Expected: the new tests FAIL (the loop is a stub; `RADIO_PASSTHROUGH` does not exist yet — compile error first).

- [ ] **Step 3: Implement the loop and the pass-through seam**

`radio_serial.rs`'s real `bridge_loop` (replacing the stub):

```rust
fn bridge_loop(
    shared: Arc<Shared>,
    session_id: u64,
    request: AttachRequest,
    mut transport: Box<dyn SerialTransport>,
    mut recorder: Option<StickRecorder>,
    stop: Arc<AtomicBool>,
    lost: Arc<std::sync::Mutex<Option<String>>>,
    done: Sender<()>,
) {
    let mut decoder = Decoder::default();
    let mut buf = [0u8; 512];
    let mut last_telemetry = Instant::now();
    let mut dropped_channels = 0u64;
    let mut unknown: Vec<(u8, u64)> = Vec::new();
    let mut end: Option<String> = None;
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        match transport.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                for frame in decoder.push(&buf[..n]) {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    match frame {
                        Frame::RcChannels(ch) => {
                            let mut slot = shared.lock();
                            let Some(session) = slot.as_mut() else { break };
                            if session.id != session_id {
                                break; // a fresh load replaced the session (its Drop already asked us to stop)
                            }
                            let sticks = sticks_from_channels(&ch);
                            session.vehicle.set_sticks(&sticks);
                            session.vehicle.set_transmitter(true);
                            if let Some(rec) = recorder.as_mut() {
                                rec.row(session.vehicle.time_s(), &sticks);
                            }
                        }
                        Frame::LinkStatistics(_) => {}
                        Frame::Other { frame_type, .. } => {
                            match unknown.iter_mut().find(|(t, _)| *t == frame_type) {
                                Some((_, count)) => *count += 1,
                                None => unknown.push((frame_type, 1)),
                            }
                        }
                    }
                }
                dropped_channels += decoder.crc_errors(); // counted at each push; the delta is the port's story
            }
            Err(e) => {
                end = Some(format!("{}: {e}", request.port));
                break;
            }
        }
        if last_telemetry.elapsed() >= TELEMETRY_PERIOD {
            last_telemetry = Instant::now();
            let slot = shared.lock();
            if let Some(session) = slot.as_ref() {
                if session.id == session_id {
                    for frame in telemetry_frames(&session.vehicle) {
                        let _ = transport.write(&frame); // a failed write is the next read's error
                    }
                }
            }
        }
    }
    *lost.lock().unwrap_or_else(|e| e.into_inner()) = end;
    if let Some(recorder) = recorder {
        recorder.finish();
    }
    shared.serial_radio.store(false, Ordering::Release);
    let _ = done.send(());
}

/// CRC errors seen so far (the loop's running count — cumulative by construction of the decoder).
```

Wait — `decoder.crc_errors()` is cumulative, so the loop line above is wrong as an increment. Track the delta:

```rust
    let mut crc_errors = 0u64;
    // in the Ok(n) arm, after the frame loop:
    let total = decoder.crc_errors();
    let _new_crc_errors = total - crc_errors; // logged with the LOST event; the count itself lives in the log
    crc_errors = total;
```

**But who publishes `SERIAL_RADIO_LOST`?** The loop cannot `shared0.publish` from a `&Shared` — it can:
`Shared::publish` takes `&self` and the loop owns `Arc<Shared>`. Publish from the loop when it ends with an
error (not on a clean stop):

```rust
    if let Some(reason) = &end {
        // The transmitter goes off (decision 9): the receiver hears nothing from now on.
        if let Some(session) = shared.lock().as_mut() {
            if session.id == session_id {
                session.vehicle.set_transmitter(false);
                let time_s = session.vehicle.time_s();
                drop(shared.lock()); // publish does not need the slot lock held
                shared.publish([crate::session::event(time_s, pb::EventKind::SerialRadioLost, reason.clone())]);
            }
        }
    }
```

Careful with the borrow: re-read the guard into a fresh `let slot = shared.lock();` before
`session.vehicle.set_transmitter(false)` and capture `time_s` while held, then drop the guard, then publish.
Write it as:

```rust
    if let Some(reason) = end {
        let mut slot = shared.lock();
        if let Some(session) = slot.as_mut() {
            if session.id == session_id {
                session.vehicle.set_transmitter(false);
                let time_s = session.vehicle.time_s();
                drop(slot);
                shared.publish([crate::session::event(time_s, pb::EventKind::SerialRadioLost, reason)]);
            }
        }
    }
```

(`end: Option<String>` moved — adjust the earlier `*lost.lock()... = end;` to clone: `= end.clone()` before
this block, or restructure so `lost` is written after. The plan's intent: the handle's `lost` field carries the
reason for `DetachSerialRadio`'s caller; the event carries it for clients. Order does not matter.)

`sticks_from_channels` (per the capture's `channel_order`; the default is AETR):

```rust
/// The capture's channel order (docs/research/serial-radio.md; AETR on the maintainer's handset) into the
/// sim's sticks: channels 1-4 are the sticks, 5-8 the aux switches the sim has, 9-16 dropped (nobody asked
/// for them and the sim has no signals for them).
fn sticks_from_channels(ch: &[u16; crsf::CHANNEL_COUNT]) -> Sticks {
    Sticks {
        roll: crsf::ticks_to_stick(ch[0]),
        pitch: crsf::ticks_to_stick(ch[1]),
        yaw: crsf::ticks_to_stick(ch[2]),
        throttle: crsf::ticks_to_throttle(ch[3]),
        aux: [
            crsf::ticks_to_stick(ch[4]),
            crsf::ticks_to_stick(ch[5]),
            crsf::ticks_to_stick(ch[6]),
            crsf::ticks_to_stick(ch[7]),
        ],
    }
}
```

`telemetry_frames` (open loop: LINK_STATISTICS only; SITL: battery and flight mode too):

```rust
/// What the handset's screen reads: the simulated link's own numbers, and the FC's battery and flight mode
/// when the session has an FC (open loop reports no FC telemetry - the existing vehicle behaviour).
fn telemetry_frames(vehicle: &Vehicle) -> Vec<Vec<u8>> {
    let state = vehicle.state();
    let ls = crsf::LinkStatistics {
        uplink_rssi_1: rssi_byte(state.radio.rssi_dbm),
        uplink_rssi_2: 0, // the sim reports one uplink antenna pair value; the second stays zero
        uplink_lq: state.radio.lq_pct.round().clamp(0.0, 100.0) as u8,
        uplink_snr: state.radio.snr_db.round().clamp(-128.0, 127.0) as i8,
        active_antenna: 0,
        rf_mode: 0,
        uplink_tx_power: 0,
        downlink_rssi: 0,
        downlink_lq: state.radio.downlink_lq_pct.round().clamp(0.0, 100.0) as u8,
        downlink_snr: 0,
    };
    let mut frames = vec![crsf::telemetry_frame(crsf::LINK_STATISTICS_FRAME_TYPE, &ls.encode())];
    let fc = state.fc_telemetry;
    if fc.flight_mode.is_empty() {
        return frames; // open loop: no FC, so no battery or flight-mode frames
    }
    frames.push(battery_frame(&state));
    frames.push(crsf::flight_mode_frame(&fc.flight_mode));
    frames
}

/// The battery sensor frame from the vehicle state's own numbers: cells come from the battery model's
/// series count (the quad config), voltage/current/used from the state snapshot.
fn battery_frame(state: &VehicleState) -> Vec<u8> {
    crsf::battery_sensor_frame(&crsf::BatterySensor {
        voltage_mv: (state.battery_voltage_v * 1000.0) as u16,
        current_ma: (state.battery_current_a * 1000.0) as u16,
        capacity_used_mah: state.battery_consumed_mah.round() as u32,
        remaining_pct: (state.battery_soc * 100.0).round().clamp(0.0, 100.0) as u8,
    })
}

/// CRSF's RSSI byte: -RSSI dBm in units of 1 dBm below -28 (the antenna-independent CRSF encoding
/// Betaflight also decodes; verified against the radio tap in M3d).
fn rssi_byte(rssi_dbm: f64) -> u8 {
    let v = (-rssi_dbm - 28.0).round().clamp(0.0, 255.0) as u8;
    v
}
```

**The RSSI byte is the one place this plan cannot pin from reading:** the M3d CRSF-telemetry encode path
(`LinkStatistics::encode`'s caller in `crates/ofs-fc` or `crates/ofs-radio`) holds the *authoritative* RSSI/LQ
quantization — read it and copy its exact formula into `rssi_byte`, replacing the sketch above. Same for the
state snapshot's field names: `VehicleState` (vehicle.rs) carries what `state_msg` reads — verify
`battery_consumed_mah`/`battery_soc`'s real names and adjust. If the sim tracks rf-mode/tx-power per session,
fill `rf_mode`/`uplink_tx_power` from `ofs_radio::elrs::rf_mode_index`/`tx_power_index_mw` (they live in
`elrs.rs`, not `crsf.rs`); zero means "unspecified" to a handset and is honest. Record every such adjustment in
the commit message.

`crates/ofs-radio/src/elrs.rs`'s pass-through: in the `Inputs` struct add `passthrough: Signal<f64>`; in
`ElrsLink::new`, `passthrough: bus.signal(&names::RADIO_PASSTHROUGH)` beside `tx_enabled`; in `step`, beside the
`transmitting` line:

```rust
        let passthrough = bus.get(self.inputs.passthrough) > 0.5;
```

and the two delivery lines become:

```rust
        let received = transmitting
            && (passthrough || (draw >= packet_error_rate(rssi[active], sensitivity) && fault_draw >= fraction && !in_burst));
```

```rust
        let downlink_received = transmitting && (passthrough || downlink_draw >= packet_error_rate(downlink[downlink_active], sensitivity));
```

(The draws keep their fixed positions and counts — M4b ruling 7. The LQ/RSSI stats flow from the packet
history, so a pass-through link reads LQ 100 and geometric RSSI, exactly what the HUD and the handset then
show.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --locked && cargo test -p ofs-radio --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS, clean — including the new tests and every determinism test (the pass-through signal defaults to
0, so nothing changes for existing runs; the bus digest re-baselining, if any, is mechanical and said so).

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-core crates/ofs-radio crates/ofs-sim
git commit -m "feat(sim): the bridge's data plane - channels to the bus, telemetry home, lost ports; ELRS pass-through

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 7: The radio CSV — recording a real flight for deterministic replay

Spec §5. `record=true` writes rows as frames are applied; the format is `play_inputs`' own.

**Files:**
- Modify: `crates/ofs-sim/src/radio_serial.rs` (`StickRecorder::row`; the detach path carries the recording
  path)
- Modify: `crates/ofs-sim/src/server.rs` (the detach event names the recording)
- Test: `crates/ofs-sim/src/radio_serial.rs`

**Interfaces:**
- Produces: `StickRecorder::row(&mut self, time_s: f64, sticks: &Sticks)`; `SerialRadioHandle.recording_path:
  Option<PathBuf>` (set at spawn, read by the detach handler's event); the `SERIAL_RADIO_DETACHED` message
  naming the CSV when one was written.

- [ ] **Step 1: Write the failing test**

Append to the test module:

```rust
    #[test]
    fn a_recorded_radio_csv_is_a_valid_play_inputs_file() {
        let (shared, id) = rigged(RunMode::Realtime);
        let logs = tmp_logs();
        let (transport, in_tx, _out) = duplex();
        let request = AttachRequest { port: "TEST".into(), baud: 0, pass_through: false, record: true };
        let (_handle, path) = spawn_bridge_with(shared.clone(), logs.clone(), "quad".into(), id, request, Box::new(transport)).unwrap();
        let path = path.expect("record=true names the file");
        let mut ticks = center();
        ticks[3] = crsf::throttle_ticks(0.5);
        in_tx.send(rc_frame(ticks)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        // A pause between frames: the CSV's simulated t_s repeats, and the file must still parse (Review Focus 5).
        if let Some(s) = shared.lock().as_mut() {
            s.pause();
        }
        in_tx.send(rc_frame(center())).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(in_tx);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while shared.serial_radio.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "the bridge never ended");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed = crate::inputs::parse(path.display().to_string().as_str(), &text);
        let parsed = parsed.expect("the radio CSV is a valid play_inputs file");
        assert_eq!(parsed.columns.len(), 8, "all eight stick columns");
        assert!(parsed.rows.len() >= 2, "two frames became two rows: {}", parsed.rows.len());
        // Rows are non-decreasing in t_s even across the pause.
        let times: Vec<f64> = parsed.rows.iter().map(|r| r.t_s).collect();
        assert!(times.windows(2).all(|w| w[1] >= w[0]), "t_s is simulated time: {times:?}");
        std::fs::remove_dir_all(logs).ok();
    }
```

(`crate::inputs::parse` is `crates/ofs-sim/src/inputs.rs`'s validator — its row struct's field names
(`t_s`) must be checked when writing; `s.pause()` — verify the Session's pause method name, else set
`running = false` through the existing API the tests use.)

- [ ] **Step 2: Run it to see the failure**

Run: `cargo test -p ofs-sim --lib radio_serial --locked a_recorded_radio_csv`
Expected: FAIL — `StickRecorder::row` does not exist yet (or writes no rows).

- [ ] **Step 3: Implement**

`StickRecorder::row`:

```rust
    fn row(&mut self, time_s: f64, sticks: &Sticks) {
        use std::io::Write;
        // One row per applied frame, M4c's columns and sample-and-hold semantics on replay; t_s is simulated
        // time, so a pause between frames shows up as repeated timestamps (and replays as a hold).
        let _ = writeln!(
            self.file,
            "{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6}",
            time_s, sticks.roll, sticks.pitch, sticks.yaw, sticks.throttle,
            sticks.aux[0], sticks.aux[1], sticks.aux[2], sticks.aux[3],
        );
    }
```

`SerialRadioHandle` gains `pub(crate) recording_path: Option<PathBuf>` (set in `spawn_bridge_with`); the detach
handler's `recording` becomes `handle.recording_path.clone()`, and the DETACHED event's message becomes:

```rust
                        let message = match &handle.recording_path {
                            Some(path) => format!("{} (recorded to {})", handle.port, path.display()),
                            None => handle.port.clone(),
                        };
```

The attach event already names the recording (Task 5's handler).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ofs-sim --locked` and clippy.
Expected: PASS, clean.

- [ ] **Step 5: Commit**

```bash
git add crates/ofs-sim
git commit -m "feat(sim): record=true writes the radio's stick stream as a play_inputs CSV

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 8: The client surfaces — Python and the Rust client the game uses

Spec §4. `python/ofs` gains the flat methods; `ofs-client` gains the Commands the Godot extension sends.

**Files:**
- Modify: `python/ofs/client.py` (`attach_radio`, `detach_radio`; `list_serial_ports` as a module function)
- Modify: `python/ofs/__init__.py` (export `list_serial_ports`)
- Modify: `crates/ofs-client/src/model.rs` (`Command::AttachSerialRadio`, `Command::DetachSerialRadio`,
  `Command::ListSerialPorts`; `Update::SerialPorts`)
- Modify: `crates/ofs-client/src/worker.rs` (the three command arms, following `Command::SetRadioLoss`'s shape)
- Test: `python/tests/test_client.py` (the gRPC-free parts), `python/tests/test_serial_radio.py` (Task 9 runs
  the real path)

**Interfaces:**
- Produces (Python): `Sim.attach_radio(port, baud=0, pass_through=False, record=False) -> str | None` (the
  recording path), `Sim.detach_radio() -> None`, `ofs.list_serial_ports() -> list[SerialPort]` where
  `SerialPort` is a small dataclass (`name`, `description`).
- Produces (Rust): `Command::AttachSerialRadio { port: String, pass_through: bool }` (the game uses the
  default baud), `Command::DetachSerialRadio`, `Command::ListSerialPorts`,
  `Update::SerialPorts(Vec<(String, String)>)`.

- [ ] **Step 1: Write the failing Python tests**

Append to `python/tests/test_client.py` (its existing tests use the grpc test server helpers in
`crates/ofs-sim/tests/grpc.rs`'s shape — for the Python side, mirror the nearest existing method test; the
attach-refusal paths are asserted here, the happy path is Task 9's pty):

```python
def test_attach_radio_refuses_without_a_port_and_detach_is_a_noop(sim):
    sim.load(QUAD)  # lockstep: the attach is refused before anything else
    with pytest.raises(ofs.ConfigError, match="lockstep"):
        sim.attach_radio(port="NOPE")
    sim.detach_radio()  # a no-op when nothing is attached


def test_list_serial_ports_returns_a_list(sim):
    ports = ofs.list_serial_ports()
    assert isinstance(ports, list)
    for p in ports:
        assert p.name
        assert isinstance(p.description, str)
```

(`ofs.ConfigError` — check the exported error name in `python/ofs/errors.py`; the server maps lockstep refusal
to `FailedPrecondition`, which `from_rpc_error` raises as whatever the client's mapping names — read it and
match the real exception; the `match=` string matches the server's message, "lockstep" is in it.)

- [ ] **Step 2: Run them to see the failures**

Run: `python -m pytest python/tests/test_client.py -q -k "attach_radio or list_serial"`
Expected: FAIL — `AttributeError` (`attach_radio`/`list_serial_ports` do not exist).

- [ ] **Step 3: Implement the Python methods**

In `python/ofs/client.py`, in the `Sim` class (beside `stop_inputs`), in the client's own doc voice:

```python
    def attach_radio(self, port: str, baud: int = 0, pass_through: bool = False, record: bool = False) -> str | None:
        """Attaches a real radio (an ELRS handset or TX module in its simulator mode) as this session's stick
        source, through the simulated ELRS link unless `pass_through`. Real-time sessions only: a live port
        cannot fly lockstep (it is not reproducible) — record the flight and replay it instead. With
        `record=True` the stick stream is written as a `play_inputs` CSV and its path is returned. A pilot, a
        playing inputs file and the radio exclude each other."""
        reply = self._call(self._stub.AttachSerialRadio, pb.AttachSerialRadioRequest(
            port=port, baud=baud, pass_through=pass_through, record=record))
        return reply.recording_path or None

    def detach_radio(self) -> None:
        """Detaches the serial radio; a no-op when nothing is attached. The RC source goes silent, so
        Betaflight failsafes as if the radio had been switched off."""
        self._call(self._stub.DetachSerialRadio, pb.Empty())
```

and the module-level function (beside `launch`):

```python
def list_serial_ports() -> list[SerialPort]:
    """The machine's serial ports (USB serial devices are radio candidates)."""
    with _anonymous_server_stub() as stub:  # replace with the file's existing pattern for a stubless call:
        ...
```

**The module-level `list_serial_ports` needs a channel without a loaded session — read how `launch`/`Sim`
build their stub and either (a) add a `Sim.list_serial_ports()` method on the same stub (simplest, and the
game's flow has a `Sim` anyway), or (b) if a module function is wanted, factor the channel construction the way
`launch` already does it. The plan's ruling: a `Sim` method** (`sim.list_serial_ports()`), and
`ofs.__init__` re-exports nothing new. Adjust the test above accordingly (`sim.list_serial_ports()`), and the
spec's `ofs.list_serial_ports()` wording is satisfied by the same call being one line away — note the deviation
in the commit message (the spec's §4 names the module function; the client has no stubless path, so the method
is where it lives).

```python
    def list_serial_ports(self) -> list[SerialPort]:
        """The machine's serial ports (`SerialPort(name, description)`); USB serial devices are radio
        candidates."""
        reply = self._call(self._stub.ListSerialPorts, pb.Empty())
        return [SerialPort(name=p.name, description=p.description) for p in reply.ports]
```

with the dataclass beside the other small types (`RadioLink`, `FcTelemetry`…):

```python
@dataclasses.dataclass(frozen=True)
class SerialPort:
    """A serial port the machine offers (`sim.list_serial_ports`)."""
    name: str
    description: str
```

(check whether the file imports `dataclasses`; the existing small types are hand-written classes — follow that
style instead if so.)

- [ ] **Step 4: Implement the Rust client's commands**

`crates/ofs-client/src/model.rs`:

```rust
pub enum Command {
    Pause,
    Resume,
    Reload,
    SetRadioLoss(bool),
    /// Attaches the serial radio (M5): the game's stick source becomes the handset.
    AttachSerialRadio { port: String, pass_through: bool },
    /// Detaches it; a no-op without one.
    DetachSerialRadio,
    /// Asks for the machine's serial ports; the reply arrives as `Update::SerialPorts`.
    ListSerialPorts,
}
```

and in `Update`:

```rust
    /// The machine's serial ports, from `Command::ListSerialPorts` (M5).
    SerialPorts(Vec<(String, String)>),
```

`crates/ofs-client/src/worker.rs`, in the command `match` beside `Command::SetRadioLoss` (the `client` here is
the worker's gRPC client wrapper — check what it exposes and add the three thin methods if it only wraps a
subset, following its existing method shapes):

```rust
                Some(Command::AttachSerialRadio { port, pass_through }) => {
                    if let Err(status) = client.attach_serial_radio(pb::AttachSerialRadioRequest {
                        port, baud: 0, pass_through, record: true,
                    }).await {
                        shared.error(status_error(status));
                    }
                }
                Some(Command::DetachSerialRadio) => {
                    if let Err(status) = client.detach_serial_radio(pb::Empty {}).await {
                        shared.error(status_error(status));
                    }
                }
                Some(Command::ListSerialPorts) => match client.list_serial_ports(pb::Empty {}).await {
                    Ok(reply) => shared.update(Update::SerialPorts(
                        reply.into_inner().ports.into_iter().map(|p| (p.name, p.description)).collect(),
                    )),
                    Err(status) => shared.error(status_error(status)),
                },
```

(The game records by default: `record: true` — a real flight is only useful replayed. `shared.update(...)`
— verify the exact method that pushes an `Update` to the poll queue; `shared.error` shows the pattern.)

`client` here is `crates/ofs-client/src/client.rs`'s gRPC wrapper: add the three methods beside
`inject_fault`/`clear_faults` (check their names), each `self.stub.<rpc>(request).await` with the wrapper's
error type.

- [ ] **Step 5: Run the tests**

Run: `python -m pytest python/tests/test_client.py -q` (after `cargo build -p ofs-sim`), then
`cargo test -p ofs-client --locked` and clippy.
Expected: PASS, clean.

- [ ] **Step 6: Commit**

```bash
git add python crates/ofs-client
git commit -m "feat(client): attach_radio/detach_radio and the port list on Python and the game's client

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 9: The fake handset end to end over a pty (Linux CI, no hardware)

Spec §6's fake-peer tier. A Python fake replays the capture's frames into a real pty the server opens.

**Files:**
- Create: `python/tests/test_serial_radio.py`
- Test: itself (posix-only)

**Interfaces:**
- Consumes: everything built so far, through the public Python API only.
- Produces: the CI's end-to-end proof that a serial peer can fly a session, and the unplug semantics.

- [ ] **Step 1: Write the test**

```python
"""The real-radio bridge end to end over a pty (M5, spec §6): a fake handset replays the capture's frames.

Posix only (a pty pair is the fake peer; Windows has no pty and the bridge's unit tests + the maintainer's
manual session cover it there). Needs no SITL: an open-loop session proves the plumbing; the failsafe
timing is the live suite's (test_sitl_radio.py)."""
import json
import os
import pathlib
import pty
import sys
import time

import pytest

import ofs
from conftest import QUAD

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="a pty pair is the fake peer (posix only)")

FIXTURES = pathlib.Path(__file__).resolve().parents[2] / "crates" / "ofs-radio" / "tests" / "fixtures" / "serial-radio"


class FakeHandset:
    """The radio side of a pty pair: writes frames like the capture shows, reads what the bridge answers."""

    def __init__(self, cap_lines):
        self.master, self._slave = pty.openpty()
        self.name = os.ttyname(self._slave)  # what attach_radio(port=...) takes
        self.cap_lines = cap_lines

    def send_window(self, t0, t1, limit=None):
        """Writes every captured tx2host frame whose t_ms is in [t0, t1]; returns the count."""
        sent = 0
        for line in self.cap_lines:
            v = json.loads(line)
            if v["dir"] != "tx2host" or not t0 <= v["t_ms"] <= t1:
                continue
            os.write(self.master, bytes.fromhex(v["hex"]))
            sent += 1
            if limit is not None and sent >= limit:
                break
        return sent

    def read_reply(self, timeout=1.0):
        import select
        end = time.time() + timeout
        data = b""
        while time.time() < end:
            r, _, _ = select.select([self.master], [], [], 0.1)
            if r:
                data += os.read(self.master, 512)
        return data

    def unplug(self):
        os.close(self.master)

    def close(self):
        for fd in (self.master, self._slave):
            try:
                os.close(fd)
            except OSError:
                pass


def _decode_crsf_frames(data):
    """Pure-python CRSF framing, the same rules as the Rust codec: sync address, length, CRC8-DVB-S2."""
    frames = []
    i = 0
    while i < len(data):
        if data[i] not in (0xC8, 0xEA, 0xEC, 0xEE):
            i += 1
            continue
        if i + 2 > len(data):
            break
        length = data[i + 1]
        if not 2 <= length <= 62 or i + length + 2 > len(data):
            i += 1
            continue
        body = data[i + 2:i + length + 1]
        crc = 0
        for b in body:
            crc ^= b
            for _ in range(8):
                crc = (crc >> 1) ^ 0xD5 if crc & 1 else crc >> 1
        if crc == data[i + length + 2]:
            frames.append((data[i], body[0], body[1:]))
        i += length + 2
    return frames


@pytest.fixture
def handset():
    lines = (FIXTURES / "capture.jsonl").read_text().splitlines()
    h = FakeHandset(lines)
    yield h
    h.close()


def _window(t0_name, t1_name=None):
    """A [t0, t1] pair from summary.toml's [windows] table (e.g. _window("throttle"))."""
    import re
    text = (FIXTURES / "summary.toml").read_text()
    key = t0_name if t1_name is None else None
    m = re.search(rf'{t0_name} = \[(\d+), (\d+)\]', text)
    assert m, f"summary.toml has no {t0_name} window: Task 1's capture is incomplete"
    return int(m.group(1)), int(m.group(2))


def test_a_captured_flight_flies_again_over_a_pty(sim, handset):
    sim.load(QUAD, mode="realtime", open_loop_fc=True)
    sim.start()
    # The capture's own throttle-window frames: the open-loop motors follow the captured throttle.
    t0, t1 = _window("throttle")
    sent = handset.send_window(t0, t1)
    assert sent > 20, f"the throttle window holds frames: {sent}"
    deadline = time.time() + 5
    motors = 0.0
    while time.time() < deadline:
        motors = max(sim.state().motor_cmd)
        if motors > 0.2:
            break
        time.sleep(0.05)
    assert motors > 0.2, f"the captured throttle never reached the motors: {motors}"

    # One stick owner: the radio holds the sticks, so the RPC paths refuse.
    with pytest.raises(ofs.SimError):
        sim.set_sticks(throttle=0.5)
    with pytest.raises(ofs.SimError):
        sim.play_inputs(_any_inputs_csv())

    # Telemetry home: the bridge answers with the sim's own numbers, CRC-valid frames to the radio address.
    data = handset.read_reply(timeout=2.0)
    frames = _decode_crsf_frames(data)
    links = [body for (addr, ftype, body) in frames if addr == 0xEC and ftype == 0x14]
    assert links, f"no LINK_STATISTICS frame in the replies: {len(frames)} frames"
    lq = links[0][2]  # payload: rssi1, rssi2, uplink_lq, ...
    assert 0 <= lq <= 100, f"the LQ byte is a percent: {lq}"
    with pytest.raises(ofs.SimError):
        sim.attach_radio(port=handset.name)  # a second attach is refused


def _any_inputs_csv():
    return "t_s,roll\n0.0,0.0\n"  # the refusal is the assertion, not the file


def test_unplugging_the_handset_fails_the_link_and_publishes_lost(sim, handset):
    sim.load(QUAD, mode="realtime", open_loop_fc=True)
    sim.start()
    assert handset.send_window(0, 10000) > 0, "the idle window holds frames"
    time.sleep(0.3)
    assert sim.state().radio.link_up, "the captured frames brought the link up"
    handset.unplug()
    events = []
    deadline = time.time() + 5
    while time.time() < deadline:
        events = [e for e in sim.events if e.kind == "serial_radio_lost"]
        if events:
            break
        time.sleep(0.05)
    assert events, "no serial_radio_lost event arrived"
    assert not sim.state().radio.link_up, "the link is down once the radio is gone"
```

(Names to verify against the client while writing: `ofs.SimError` — the exported error type the RPC refusals
raise (read `python/ofs/errors.py`'s mapping of `InvalidArgument`/`FailedPrecondition` and use whatever it
raises); `sim.events`'s real accessor (a method or property — call it accordingly) and the Python-side
event-kind strings (`_kind_name` in `python/ofs/client.py` grows the three new kinds); `State.motor_cmd` is the
open-loop throttle observable — `pb::State` deliberately carries no sticks, clients see the world, not the
wire.)

- [ ] **Step 2: Run it**

On WSL (the recipe at the top; the pty needs Linux):
`... python -m pytest -q python/tests/test_serial_radio.py`
Expected: PASS. If the server cannot open the pty slave (serialport on a pty needs the port to accept
termios — it does on Linux), the failure says so: capture the error, and if serialport refuses ptys, the plan
records the deviation and the pty tier moves to a socat-created pty pair (`socat -d -d pty,raw,echo=0 pty,raw,echo=0`)
with the two names passed to the test — try the direct pty first.

- [ ] **Step 3: Add it to CI**

In `.github/workflows/ci.yml`, the Linux core job's pytest line gains `python/tests/test_serial_radio.py`
(the job runs on Linux, has no SITL — the test needs none). Check the job's existing file list and append in
alphabetical position.

- [ ] **Step 4: Commit**

```bash
git add python/tests/test_serial_radio.py .github/workflows/ci.yml
git commit -m "test(radio): the fake handset end to end over a pty - captured frames fly, unplugging is a failsafe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 10: A recorded radio flight replays deterministically

Spec §5. The whole chain pinned: real radio → record → `play_inputs` in lockstep → identical MCAP.

**Files:**
- Modify: `python/tests/test_recording.py`

**Interfaces:**
- Consumes: Task 7's CSV, Task 9's pty handset, `sim.play_inputs`, `sim.log.path`, `_sha` (existing helper).

- [ ] **Step 1: Write the test**

Append to `python/tests/test_recording.py` (posix-only, like Task 9 — it needs a pty to record from):

```python
@pytest.mark.skipif(sys.platform == "win32", reason="records from a pty fake handset (posix only)")
def test_a_recorded_radio_flight_replays_byte_identically(sim, tmp_path):
    """M5 (spec §5): a real-radio session records its sticks as a play_inputs CSV; two lockstep replays of
    that CSV write identical MCAP files."""
    import os, pty, time, json
    master, slave = pty.openpty()
    try:
        sim.load(QUAD, mode="realtime", open_loop_fc=True)
        sim.start()
        path = sim.attach_radio(port=os.ttyname(slave), record=True)
        assert path, "record=True returns the CSV path"
        for line in (FIXTURES / "capture.jsonl").read_text().splitlines()[:30]:
            v = json.loads(line)
            if v["dir"] == "tx2host":
                os.write(master, bytes.fromhex(v["hex"]))
        time.sleep(0.5)
        sim.detach_radio()
        sim.pause()
    finally:
        os.close(master)
        os.close(slave)

    text = pathlib.Path(path).read_text()
    assert len(text.splitlines()) > 5, "the capture's frames became rows"

    hashes = []
    for _ in range(2):
        sim.load(QUAD, seed=5, mode="lockstep", open_loop_fc=True)
        sim.play_inputs(path)
        sim.run(2.0)
        hashes.append(_sha(sim.log.path))
    assert hashes[0] == hashes[1], "two replays of the same radio CSV differ"
```

(`FIXTURES` — reuse Task 9's module-level constant by importing or re-deriving it here; `sim.pause()` — check
the client's pause method name; `_sha` and `sim.log.path` exist.)

- [ ] **Step 2: Run it**

On WSL: `... python -m pytest -q python/tests/test_recording.py -k radio_flight`
Expected: PASS. Check it bites: temporarily corrupt the second replay's CSV (append a row), see the hash
assertion fail, restore.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_recording.py
git commit -m "test(recording): a recorded radio flight replays byte-identically in lockstep

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 11: Live SITL — failsafe on unplug, telemetry with Betaflight's numbers

Spec §6's live tier and the exit criteria's failsafe clause.

**Files:**
- Modify: `python/tests/test_sitl_radio.py`

**Interfaces:**
- Consumes: Task 9's `FakeHandset` (import it or duplicate it — the file must stay self-contained per the
  repo's test style: duplicate the class with a comment naming its origin), `OFS_SITL_LAUNCH` gating (existing
  `pytestmark`).

- [ ] **Step 1: Write the tests**

Append to `python/tests/test_sitl_radio.py` (these attach to a live port, so they are **real-time sessions** —
`mode="realtime"` and `sim.start()`; the existing tests in this file are lockstep and stay as they are). The
file gains the fixture imports from Task 9 (`from test_serial_radio import FIXTURES` — same directory) and
`import tomllib` (Python 3.11+) or a two-line manual parse of `summary.toml` if the floor is 3.10: the repo's
Python floor is 3.10, so parse the one integer pair with `re` — no new dependency):

```python
def _armed_frames():
    """The capture's own frames from the switch window's second half: the maintainer's switches moved there,
    so those frames carry a real, valid channel state to arm with (no hand-built packing in the tests)."""
    import json, re
    text = (FIXTURES / "summary.toml").read_text()
    m = re.search(r'switches = \[(\d+), (\d+)\]', text)
    assert m, "summary.toml has no switches window: Task 1's capture is incomplete"
    t0, t1 = int(m.group(1)), int(m.group(2))
    frames = []
    for line in (FIXTURES / "capture.jsonl").read_text().splitlines():
        v = json.loads(line)
        if v["dir"] == "tx2host" and t0 + (t1 - t0) // 2 <= v["t_ms"] <= t1:
            frames.append(bytes.fromhex(v["hex"]))
    assert frames, "the switch window holds no frames: re-run Task 1's capture with an arm-and-disarm pass"
    return frames


def test_unplugging_the_radio_failsafes_within_betaflights_timing(sim):
    """M5 exit criteria: the cable pulled (or the radio switched off) is a receiver gone silent - Betaflight
    failsafes on its normal schedule, exactly like the K key's link loss."""
    import os, pty, time
    master, slave = pty.openpty()
    try:
        sim.load(QUAD, seed=2, mode="realtime")
        sim.start()
        time.sleep(4.0)  # boot and gyro calibration
        sim.attach_radio(port=os.ttyname(slave))
        frames = _armed_frames()
        deadline = time.time() + 5.0
        i = 0
        while time.time() < deadline:
            os.write(master, frames[i % len(frames)])
            i += 1
            time.sleep(0.02)  # the handset's own packet rate
            s = sim.state()
            if min(s.motor_cmd) > 0.0:
                break
        else:
            pytest.fail(f"Betaflight never armed from the capture's frames: {sim.state().motor_cmd}")
        os.close(master)  # the cable is pulled; the bridge sees the port die
        cut = sim.state().time_s
        while sim.state().time_s < cut + 4.0 and max(sim.state().motor_cmd) > 0.0:
            time.sleep(0.02)
        s = sim.state()
        assert max(s.motor_cmd) == 0.0, "Betaflight never disarmed after the unplug"
        assert 1.4 <= s.time_s - cut <= 2.2, s.time_s - cut
        assert not s.radio.link_up
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        try:
            os.close(slave)
        except OSError:
            pass


def test_pass_through_delivers_sticks_without_the_link_model(sim):
    import os, pty, time
    master, slave = pty.openpty()
    try:
        sim.load(QUAD, seed=3, mode="realtime")
        sim.start()
        time.sleep(4.0)
        sim.attach_radio(port=os.ttyname(slave), pass_through=True)
        frames = _armed_frames()
        deadline = time.time() + 3.0
        i = 0
        while time.time() < deadline:
            os.write(master, frames[i % len(frames)])
            i += 1
            time.sleep(0.02)
        s = sim.state()
        assert s.radio.lq_pct == 100.0, f"pass-through delivers every packet: {s.radio}"
    finally:
        try:
            os.close(master)
        except OSError:
            pass
        try:
            os.close(slave)
        except OSError:
            pass
```

**If Betaflight never arms from the switch-window frames** (throttle held above idle, or the maintainer's
switch pass never touched the arm switch), the capture lacks what arming needs: extend Task 1's capture
checklist with an explicit "arm and disarm once (throttle at idle, arm switch high)" window, re-run Task 1's
Steps 2–3, and use that window here instead. That is the capture-first contract working as designed: the test
flies what the maintainer's handset actually sent, never a synthetic frame.

- [ ] **Step 2: Run it live**

On WSL (the recipe at the top; the pty tests run there too — one SITL at a time):
`... python -m pytest -q python/tests/test_sitl_radio.py`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_sitl_radio.py
git commit -m "test(sitl): unplugging the radio failsafes on Betaflight's own timing; pass-through smoke

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Task 12: The game's Radio section, and the docs

Spec §4's Godot surface and the closing docs. The maintainer's manual session runs at the end.

**Files:**
- Modify: `crates/ofs-client/src/model.rs`, `src/worker.rs` (already done in Task 8 — this task consumes them)
- Modify: `crates/ofs-godot/src/lib.rs` (`list_serial_ports`, `radio_attach`, `radio_detach` functions)
- Modify: `godot/ui/controls_menu.gd` (the Radio section), `godot/scripts/app.gd` (the wiring and persistence)
- Modify: `godot/tests/test_controls_menu.gd` (the section's tests)
- Modify: `README.md`, `docs/dev-setup.md`

**Interfaces:**
- Consumes: `Command::AttachSerialRadio`/`DetachSerialRadio`/`ListSerialPorts`, `Update::SerialPorts`
  (Task 8), the game's existing command plumbing (`set_radio_loss`'s GDScript → `Command` → HUD pattern).
- Produces: the F2 screen's Radio section (port dropdown + refresh + attach/detach + pass-through toggle,
  persisted in `user://controls.json`), `OfsClient.list_serial_ports()`/`radio_attach(port, pass_through)`/
  `radio_detach()` on the gdext side, and the docs.

- [ ] **Step 1: The gdext functions**

In `crates/ofs-godot/src/lib.rs`, beside the existing `set_radio_loss` (find it — the K key's plumbing),
following its exact shape for command sends:

```rust
    /// The machine's serial ports (M5): `[{name, description}]`, for the controls screen's dropdown.
    #[func]
    pub fn list_serial_ports(&mut self) -> Array<Dictionary> {
        self.send_command(Command::ListSerialPorts);
        Array::new() // the reply arrives as Update::SerialPorts; see radio_poll below
    }

    /// Attaches the serial radio as the stick source (M5). Returns "" on success or the error text.
    #[func]
    pub fn radio_attach(&mut self, port: String, pass_through: bool) -> String {
        self.send_command_blocking(Command::AttachSerialRadio { port: port.clone(), pass_through })
    }

    /// Detaches it; a no-op without one. Returns "" on success or the error text.
    #[func]
    pub fn radio_detach(&mut self) -> String {
        self.send_command_blocking(Command::DetachSerialRadio)
    }
```

**Read how `dvr_start` (the existing error-returning command) gets its reply back to GDScript and mirror
that mechanism for `radio_attach`/`radio_detach`** — if the client's worker model is fire-and-forget with
polled Updates, then attach/detach results must arrive as Updates too: add
`Update::RadioResult { attached: bool, error: String }` in Task 8's model (do it now if Task 8 shipped them as
blocking calls and the worker turned out to be async-only), and the GDScript polls it the way it polls
telemetry. Whichever shape the existing code dictates, the commit message records the choice. `Update::SerialPorts`
feeds the dropdown through the same poll.

- [ ] **Step 2: The F2 Radio section (GDScript)**

In `godot/ui/controls_menu.gd`, after the aspect row (before `_hint`), build the section the way the preset
row is built:

```gdscript
	var radio_row := HBoxContainer.new()
	box.add_child(radio_row)
	var radio_label := Label.new()
	radio_label.text = "Radio"
	radio_row.add_child(radio_label)
	_radio_port.custom_minimum_size.x = 220
	radio_row.add_child(_radio_port)
	var radio_refresh := Button.new()
	radio_refresh.text = "Refresh"
	radio_refresh.pressed.connect(_refresh_radio_ports)
	radio_row.add_child(radio_refresh)
	_radio_attach.text = "Attach"
	_radio_attach.pressed.connect(_on_radio_attach)
	radio_row.add_child(_radio_attach)
	_radio_passthrough.text = "Pass through the link model (pure input)"
	_radio_passthrough.toggled.connect(func(on: bool) -> void: _radio_passthrough_state = on)
	radio_row.add_child(_radio_passthrough)
```

with the fields beside `_aspect`:

```gdscript
var _radio_port := OptionButton.new()
var _radio_attach := Button.new()
var _radio_passthrough := CheckBox.new()
var _radio_passthrough_state := false
```

and the handlers + persistence (the section saves into the same `controls.json` the bindings use — a
`"radio"` key):

```gdscript
## Asks the client for the machine's serial ports and refills the dropdown.
func _refresh_radio_ports() -> void:
	_radio_port.clear()
	for p in client_of().list_serial_ports():
		_radio_port.add_item("%s  (%s)" % [p["name"], p["description"]] if p["description"] != "" else p["name"])
		_radio_port.set_item_metadata(_radio_port.item_count - 1, p["name"])


func _on_radio_attach() -> void:
	if _radio_attached:
		var err := client_of().radio_detach()
		_set_radio_attached(err == "")
	else:
		var port: String = _radio_port.get_item_metadata(_radio_port.selected) if _radio_port.item_count > 0 else ""
		if port == "":
			_hint.text = "No port selected: Refresh first."
			return
		var err := client_of().radio_attach(port, _radio_passthrough_state)
		_hint.text = err if err != "" else "Radio attached: %s flies now." % port
		_set_radio_attached(err == "")
	_save()


func _set_radio_attached(on: bool) -> void:
	_radio_attached = on
	_radio_attach.text = "Detach" if on else "Attach"


## The client the game drives (the same object app.gd holds); the screen receives it in setup().
```

(`client_of()` — the screen has no client reference today: extend `setup(mapper, path)` to
`setup(mapper, path, client)` with the game passing its `OfsClient` node, updating `app.gd`'s call site and the
test helper that builds the menu. The attached state persists as `"radio": {"port": ..., "pass_through": ...,
"attached": bool}` in `controls.json`, restored by `app.gd` at startup **only as dropdown state** — attaching
happens on button press, never automatically (a session must exist first, and auto-attaching hardware on
launch would surprise). `_save()` gains the radio key; the load side reads it if present.)

- [ ] **Step 3: The failing Godot tests, then the wiring**

In `godot/tests/test_controls_menu.gd`, add:

```gdscript
func test_the_radio_section_lists_ports_and_toggles_attach() -> void:
	var menu: Node = ControlsMenu.new()
	await add_to_tree(menu)
	menu.setup(_mapper(), SAVE_PATH, _fake_client())  # a stand-in client exposing list_serial_ports/radio_attach/radio_detach
	menu._refresh_radio_ports()
	eq(menu._radio_port.item_count, 2, "the stand-in's two ports are listed")
	menu._radio_port.select(0)
	menu._on_radio_attach()
	eq(menu._radio_attach.text, "Detach", "the button flips once attached")
	menu._on_radio_attach()
	eq(menu._radio_attach.text, "Attach", "and flips back")
	var saved := FileAccess.get_file_as_bytes(SAVE_PATH)
	ok(saved.size() > 0, "the radio state persisted")
```

(`_fake_client()` — a small RefCounted with the three methods returning canned values, following the
`MockClient` precedent in `godot/tests/test_dvr.gd`; `_mapper()` — the existing test file's helper for a
controls mapper.) Run:

`GODOT_BIN=C:/tools/godot/Godot_v4.7.2-stable_win64_console.exe bash scripts/run-godot-tests.sh unit`

Expected: FAIL first (no `client` parameter on `setup`, no radio fields), then PASS after Steps 1–2. Update
`app.gd`'s `controls_menu.setup(controls, CONTROLS_PATH)` call and `test_controls_menu.gd`'s other tests'
`setup` calls to the new signature in the same commit.

In `app.gd`: handle `Update::SerialPorts`/`Update::RadioResult` in the poll loop the way other Updates are
handled (refresh the dropdown's contents when the screen is open; surface an attach error in the HUD banner
the way a failed Pause is surfaced).

- [ ] **Step 4: Run the suites**

Run: `bash scripts/run-godot-tests.sh unit` and `bash scripts/run-godot-tests.sh e2e`; then
`cargo test --workspace --locked` and clippy.
Expected: PASS, clean.

- [ ] **Step 5: The docs**

`README.md`: in the Godot client section, after the keys list, a paragraph in the file's voice: a real ELRS
handset (or TX module) in simulator mode flies the quad — put the module in simulator mode, plug it in, F2 →
Radio → Refresh → pick the port → Attach; the simulated link stays in the path (failsafe on radio-off), the
handset's telemetry screen shows the simulated link's numbers, and Attach with "Pass through" bypasses the
link model for pure input. Name `docs/research/serial-radio.md` as the wire's ground truth. The Python
section's scripting list gains `sim.attach_radio(...)` / `sim.detach_radio()` / `sim.list_serial_ports()`.

`docs/dev-setup.md`: the serial-port notes (Windows device naming from the capture; Linux `dialout`).

`docs/research/serial-radio.md`: a closing line per section stating what M5 shipped against it (constants as
built, any deviation from the capture and why).

- [ ] **Step 6: The maintainer's manual session (the exit criteria's live half)**

With the maintainer, the M3c–e style checklist, recorded in `docs/superpowers/history/` as
`m5-manual-session.md` (the pattern of the M3c–e notes): fly the shipped freestyle quad from the EdgeTX15
through the simulated link (arm from the radio, real sticks); failsafe by switching the radio off (timing
matches the K key's); pull the cable mid-hover (same); the handset's telemetry screen shows LQ falling with
distance and battery matching Betaflight's OSD; the DVR still records; the K key still cuts the simulated link
while attached. Every observed number goes in the note with the date.

- [ ] **Step 7: Commit**

```bash
git add crates/ofs-godot crates/ofs-client godot README.md docs/dev-setup.md docs/research/serial-radio.md docs/superpowers/history/m5-manual-session.md
git commit -m "feat(godot): the F2 Radio section; the radio bridge documented and flown for real

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Verification (the full set, before the merge)

1. Windows: `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
   `python -m pytest python/tests -q` (pty tests skip), both Godot suites.
2. WSL: the live Rust suite, then `python/tests/test_serial_radio.py`, `test_recording.py -k radio_flight`,
   and `test_sitl_radio.py` (one SITL, `--test-threads=1`).
3. CI on the branch push: green everywhere, `test_serial_radio.py` in the Linux job.
4. The manual session note exists with the maintainer's numbers.
