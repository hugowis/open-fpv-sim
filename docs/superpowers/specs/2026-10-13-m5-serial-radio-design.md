# M5: the real-radio bridge — design

Status: designed 2026-10-13; approved in conversation. Parent: the phase-2 spec
(`2026-10-13-phase2-real-gear-trusted-numbers-design.md`, §3 is this milestone's contract summary). Predecessor: the
v1 close-out (merges before M5 executes; this spec is written against the close-out branch's tree). It closes the
parent spec's oldest deferral — "joystick first; real-radio CRSF-over-serial later" — as phase 2's first milestone.

## 1. Purpose and scope

M5 lets a real radio fly the simulator: an ExpressLRS handset or TX module in its simulator mode speaks CRSF over
USB serial; the simulator reads its channel frames as RC input and sends its CRSF telemetry back to the handset's
screen. The simulated ELRS link stays in the path — a real radio flying a *simulated* RF link, with real loss,
latency, LQ and failsafe.

**Success looks like:** the maintainer plugs a RadioMaster handset (EdgeTX) into the PC, picks its port in the
Godot client's controls screen, and flies the Godot client with the radio's sticks and switches — arm from the real
radio, failsafe by switching the radio off — while the handset's telemetry screen shows the simulated link's LQ and
Betaflight's battery, dashing out when the simulated link dies. A script records the radio flight to a CSV and
replays it in lockstep deterministically. CI tests every path against a fake peer; no hardware is needed there.

### Decisions made in the design

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | **The server owns the serial port** (`ofs-sim`); clients only choose it. The bridge applies channel updates through `Vehicle::set_sticks`/`set_transmitter` — the same path and guards as the RPC. | A client-side bridge piping serial→`SetSticks`/`Pilot` RPCs (two ownership systems to keep exclusive; latency and timing client-dependent; the Python and Godot clients each reimplement it). |
| 2 | **Through the link model by default, `pass_through` as the option** (the phase ruling). Channel frames enter the ELRS model's RC bus signals, so loss, latency, LQ, failsafe all apply. | Bypass-only (a radio that cannot failsafe in the sim teaches nothing); bypass-only is kept as the opt-in for pure input use. |
| 3 | **One stick owner, extended.** The serial radio joins the existing exclusivity set: while attached it refuses `SetSticks`, `PlayInputs` and a `Pilot` stream, and the reverse (attaching while any of those is active) is refused too. `DetachSerialRadio` with none attached is a success no-op. | Per-source corner-case rules (every combination would need its own ruling forever). |
| 4 | **The bridge plays the TX side of the wire**: it answers the handset with `LINK_STATISTICS` carrying the *simulated link's* numbers, plus battery and flight-mode frames from the existing `FcTelemetryHandle`, at the cadence the capture establishes. Frames the handset sends that the bridge does not implement (LUA/MSP-over-CRSF, heartbeats) are counted per type and ignored — silently answering them would promise Betaflight-LUA support that does not exist (roadmap row, not M5). | Emulating a full TX module (Betaflight LUA on the radio drags MSP-over-CRSF in; a separate, post-M5 item). |
| 5 | **Attach is real-time only.** A lockstep session refuses attach: a live port is not reproducible by construction. Determinism comes from recording: attach takes `record`, and the server writes the stick stream as a `play_inputs`-format CSV; two lockstep replays write identical MCAP. | Lockstep-with-a-live-port (determinism would be a lie) or a bespoke radio-replay format (M4c's inputs are already the format, and `play_inputs` already enforces its semantics). |
| 6 | **Protocol version 10.** New RPCs `AttachSerialRadio`/`DetachSerialRadio`/`ListSerialPorts`, events `SERIAL_RADIO_ATTACHED/_DETACHED/_LOST` (kinds 20–22). The Python client bumps its `PROTOCOL_VERSION`. | Sneaking fields into existing messages (the handshake exists precisely to catch old clients against a new server honestly). |
| 7 | **`ofs-radio` stays I/O-free** (the `Model` trait rule). New encoders/decoders are pure functions in `crsf.rs`, golden-tested; the serial I/O and its transport seam live in `ofs-sim` (`radio_serial.rs`), on a plain thread doing blocking reads/writes against the `serialport` crate — no new async coupling. | `tokio-serial` (a second async story for one device); serial I/O in Godot only (Python users get nothing). |
| 8 | **Ground truth first:** the capture (§2) is Task 1 of the plan. The bridge is built against what the handset actually sends (baud, frame types, channel order, cadence), pinned as golden fixtures; the fake CI peer replays the capture. | Building from the CRSF spec's assumptions (sim-mode wire details vary by firmware version; the capture catches them at zero cost). |
| 9 | **Unplug = radio off.** The port dying (cable pulled, radio switched off) publishes `SERIAL_RADIO_LOST`, detaches, and leaves the RC source silent — exactly `set_transmitter(false)` — so Betaflight failsafes as if the radio went off. A deliberate detach behaves the same (a detached radio is a silent radio). | Holding last-known sticks (a sim that flies on after its pilot vanished); a distinct "detach while airborne is an error" rule (no honest way to justify it — the radio is gone either way). |

### Out of scope

Real RF (a receiver hearing radio waves — level 2); Betaflight LUA / MSP-over-CRSF to the handset (roadmap;
counted-and-ignored per decision 4); Bluetooth/Bluetooth-joystick radios; multiple simultaneous radios (one port,
one owner); non-ELRS serial protocols (SBUS/IBUS — the decoder speaks CRSF; others are roadmap rows); attach to a
session while it is unloaded (attach is per-session; a new `load` detaches — the event says so); M6 items (any
model upgrade).

## 2. Ground truth: the capture

A throwaway Python script (`spikes/` or `scripts/`, kept as a tool) records raw bytes + wall-clock timestamps from
the handset's USB port while the maintainer: wiggles each stick and switch in turn, opens EdgeTX's telemetry screen,
runs the ELRS LUA script, and switches the radio off. The capture's decoded summary lands in
`docs/research/serial-radio.md` (new), and the fixtures (raw byte files + a decoded index) land in
`crates/ofs-radio/tests/fixtures/serial-radio/`.

What it must establish (each a fact in the research doc, not an assumption):

- **Baud and framing** — expected 420000 8N1, but the capture decides; the constant is named once (`ofs-radio`).
- **Frame set and periods** — which CRSF frame types arrive (expect `RC_CHANNELS_PACKED` dominant), at what rate,
  and whether the handset transmits continuously or idles without arming.
- **Channel order** — AETR vs TAER as EdgeTX sends it, and the aux channel mapping the maintainer's model uses.
- **Telemetry display** — what EdgeTX's telemetry screen and the ELRS LUA script read (`LINK_STATISTICS`, battery
  sensor frames, flight-mode text), which fixes what the bridge must answer.
- **Port naming** on Windows (COM device description) and Linux (`/dev/ttyACM*` + udev note for
  dialout permission).

## 3. The bridge (`crates/ofs-sim/src/radio_serial.rs`)

A `SerialRadioBridge` is spawned by `AttachSerialRadio` and lives on one plain thread (blocking I/O, no async
coupling):

- **Transport seam.** The thread works against a small `SerialTransport` trait (`open`, `read`, `write`, port
  metadata) with two implementations: the real `serialport` one, and an in-memory duplex for tests and the fake
  peer. CI's Linux job can instead use a real pty pair end to end; Windows tests use the in-memory fake (Windows
  has no pty; loopback COM drivers are a machine change nobody should need).
- **Input path.** Bytes → `ofs_radio::crsf::Decoder` (already streaming, sync-skipping, CRC-counting,
  fuzz-shaped) → `RcChannels` frames → ticks → normalized channels → `Vehicle::set_sticks`, and
  `set_transmitter(true)` on the first channel frame. Channels 1–4 → roll/pitch/yaw/throttle, 5–8 → aux1–4 (order
  per the capture); channels beyond 8 are dropped and counted. `LinkStatistics`/`Other` frames from the handset go
  to the telemetry path (§4); CRC errors count per port lifetime.
- **Telemetry path.** The bridge answers at the captured cadence (default 10 Hz until the capture says otherwise):
  `LINK_STATISTICS` built from the bus's own radio signals (`radio.lq_pct`, `rssi_dbm`, `snr_db`, antenna,
  rf-mode/power — the same numbers the HUD shows), battery frames from `FcTelemetryHandle` (cells, voltage,
  current, capacity drawn from SoC), flight-mode text from the FC's flight mode. In open loop there is no FC
  telemetry (existing test: "an open loop quad reports no fc telemetry") — the bridge sends `LINK_STATISTICS` only,
  and says so in the research doc.
- **Lifecycle.** `AttachSerialRadio{port, baud, pass_through, record}` on a loaded session: no active stick source
  (else `InvalidState` naming the holder), not lockstep (else `FailedPrecondition` naming the mode), not already
  attached (one radio; else `InvalidState`). Open failure fails the RPC with the OS error naming the port. Success
  publishes `SERIAL_RADIO_ATTACHED` and an empty event stream proves exclusivity both ways (`Pilot` connect and
  `PlayInputs` while attached are refused the same way they are refused during playback). `DetachSerialRadio`
  (and `Unload`, and the port dying) stops the thread, publishes `SERIAL_RADIO_DETACHED`/`_LOST`, and the RC
  source goes silent — `set_transmitter(false)`, failsafe as usual. A fresh `load` detaches any bridge first.
- **Watch thread → server.** The thread reports channel updates through the vehicle's existing lock (the same
  `Slot` mutex `set_sticks` uses — no new synchronization design) and publishes its events through the session's
  existing event channel.

## 4. Protocol surface

**Proto (version 10):**

```proto
rpc AttachSerialRadio(AttachSerialRadioRequest) returns (AttachSerialRadioReply);
rpc DetachSerialRadio(Empty) returns (Empty);
rpc ListSerialPorts(Empty) returns (SerialPorts);

message AttachSerialRadioRequest {
  string port = 1;         // from ListSerialPorts
  uint32 baud = 2;         // 0 = the captured default (420000 until the capture says otherwise)
  bool pass_through = 3;   // skip the ELRS link model (pure input; decision 2)
  bool record = 4;         // write the stick stream as a play_inputs CSV
}
message AttachSerialRadioReply { string recording_path = 1; }  // empty unless record
message SerialPorts { repeated SerialPortInfo ports = 1; }
message SerialPortInfo { string name = 1; string description = 2; }  // OS-provided
```

Events: `EVENT_KIND_SERIAL_RADIO_ATTACHED = 20` (names port and baud), `_DETACHED = 21` (names the recording path
when one was written), `_LOST = 22` (names the port and the OS error).

**Python (`python/ofs/client.py`):** flat methods in the client's existing style — `sim.attach_radio(port,
baud=0, pass_through=False, record=False) -> str | None` (the recording path), `sim.detach_radio()`, and
`ofs.list_serial_ports() -> list[SerialPort]` (name, description). Old clients against a new server already fail
at the handshake.

**Godot:** the F2 controls screen gains a *Radio* section — a port dropdown fed by `ListSerialPorts` with a
refresh button, attach/detach buttons, the baud and a pass-through toggle, persisted in `user://controls.json`
beside the bindings. While attached, the game does not offer its own stick input (the server would refuse it);
the HUD's radio line keeps its job — it shows the simulated link the real radio is flying. On `SERIAL_RADIO_LOST`
the HUD flashes the same banner the pilot-disconnect path uses. The K key still cuts the simulated link (it cuts
the link model, not the wire).

## 5. Determinism and record/replay

- Attach refused in lockstep (decision 5). Real-time sessions record with `record=true`: one CSV row per channel
  update received, `t_s` relative to the first frame, `roll,pitch,yaw,throttle,aux1..aux4` — exactly
  `play_inputs`' format (sample-and-hold, aux columns included; rows quantized to ticks like M4c's playback).
  The file lands under `<data_dir>/logs/<quad>-radio-<n>.csv`, server-named (no client paths, as ever).
- Replay is existing machinery: `sim.play_inputs("flight.csv")` in lockstep; two replays write byte-identical
  MCAP (the M4c determinism contract already covers inputs; one test pins it for a radio CSV specifically).
- `pass_through` changes only which model consumes the channels; recording works identically.

## 6. Testing

| Tier | What | Where |
|---|---|---|
| Golden frames | The capture's fixtures decode exactly: framing survives arbitrary chunking, channel order maps as documented, the telemetry encoders produce the captured reply bytes byte-for-byte | `crates/ofs-radio/tests/crsf_golden.rs` + `tests/fixtures/serial-radio/` |
| Bridge unit (in-memory transport) | Bytes → bus channels; exclusivity both directions (attach vs SetSticks/PlayInputs/Pilot); lockstep refusal; double-attach refusal; detach/unplug → transmitter off; unknown frames counted; channels > 8 dropped; open-failure error text names the port | `crates/ofs-sim/tests/radio_serial.rs` |
| Fake peer end to end (Linux CI, pty) | A Python fake handset replays the captured traffic into a real serial device; the session flies the recorded flight; telemetry read back from the pty matches the expected frames; unplug (pty close) → failsafe in the live SITL suite | `python/tests/test_serial_radio.py` (+ `test_sitl_radio.py` grows the failsafe case) |
| Replay determinism | A recorded radio CSV replayed twice in lockstep writes identical MCAP | `python/tests/test_recording.py` |
| Manual session | The M3c–e style checklist with the EdgeTX15: fly from the radio (arm switch, rates), failsafe on radio-off and on cable-pull, telemetry on the handset's screen (LQ falls with distance, battery matches, dashes at link loss), K key still works, DVR unaffected | `docs/superpowers/history/` evidence note, linked from the roadmap |

The Godot unit/e2e suites gain the controls-screen radio section (dropdown renders, persistence round-trips) with
the in-memory fake — no port needed.

## 7. Risks

| Risk | Answer |
|---|---|
| Sim-mode wire details differ from expectations (baud, cadence, channel order) | The capture (§2) exists precisely to answer this first; the bridge's constants all trace to it. |
| EdgeTX LUA/MSP traffic confuses the bridge | Decision 4: counted per type, ignored; the capture documents exactly what the maintainer's handset sends. |
| Windows COM semantics (CDC data pipes, driver naming) | Part of the capture evidence; the maintainer's handset is the ground-truth device from day one; the pty end-to-end stays Linux-only by design. |
| Telemetry cadence mismatch (EdgeTX expect interval) | The capture measures what the ELRS LUA script reads happily; the bridge's cadence constant is a named, captured value, not a guess. |
| A noisy port stalls the sim | The `Decoder` already sync-skips, bounds its buffer and counts CRC errors; the bridge adds no unbounded buffering of its own. |
| Thread/lock contention with the runner at 8 kHz | Channel updates arrive at the handset's packet rate (≤ 500 Hz); the bridge holds the vehicle lock only for the bus writes — the same shape as the RPC path today. |
