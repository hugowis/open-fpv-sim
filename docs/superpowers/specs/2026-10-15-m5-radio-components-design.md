# M5: swappable radio components — design

Status: designed 2026-10-15; approved in conversation; **amended 2026-10-16 to what was built** (the plan's rulings, applied below). Parent: the phase-2 spec
(`2026-10-13-phase2-real-gear-trusted-numbers-design.md`, §3 is amended to point here). **Supersedes**
`2026-10-13-m5-serial-radio-design.md`, whose premise did not survive its own capture-first rule (§1).

## 1. Why this replaces the serial-radio M5

The serial-radio spec assumed an ELRS handset in "simulator mode" speaks CRSF over USB serial. Task 1's capture session
on the maintainer's RadioMaster TX15 (EdgeTX, USB `0483:5740`, 2026-10-15) found otherwise:

- In "USB Serial (VCP)" mode the radio offers only CLI, Lua and Telemetry Mirror on that port; passive reads returned
  zero bytes, and CLI answered `help`. Nothing streams the channel frames.
- Windows could not start the radio's CDC interfaces (`usbser` `CM_PROB_FAILED_START`, the second interface without a
  driver); the port only opened from Linux, via `usbipd-win` into WSL.
- The way commercial simulators read a radio is USB joystick (HID), which the project already supports.

So M5 keeps the USB joystick as the input and changes what the milestone is: **the radio becomes a set of swappable
components**, so other links and protocols can be tried, and a live flight can be recorded for deterministic replay.
No telemetry returns to a physical radio. The finding is kept as `docs/research/serial-radio.md`; a real-serial-radio
row (blocked; the TX15's module bay is the untested route) goes on `docs/roadmap.md`.

## 2. Purpose and scope

**Success looks like:** a quad file can name its radio's handset, link and receiver parts; `kind = "elrs"` flies exactly
as before (byte-identical seeded output); an `elrs` 900 MHz profile and a geometry-free `fixed` link fly through the
same chain; a test-local custom `Link` plugs in without touching the core; and any real-time session's sticks can be
recorded to a `play_inputs` CSV that two lockstep replays turn into identical MCAP.

### Decisions

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | **Three traits, one chain** (`Handset`, `Link`, `Receiver` composed by `RadioChain`); `radio.elrs` is refactored into the first implementation of each. | Two seams (handset + a combined link/receiver); parameter tables on the monolith (no place for an experimental model). |
| 2 | **Betaflight keeps seeing CRSF on a UART.** The split is internal; other FC-side protocols (SBUS, iBUS, FrSky, DSMX) are not in scope. | Per-protocol receivers now (each needs a matching Betaflight config; roadmap). |
| 3 | **Only a `Link` draws random numbers.** Handset and receiver are pure. The `elrs` link keeps the stream names `radio.elrs` and `elrs-faults` and its draw order; `fixed` uses `radio.fixed`. | Per-part RNG streams (shifts every existing seeded run). |
| 4 | **Channels are CRSF 11-bit ticks `[u16; 16]` between the parts**; the handset quantizes with today's `stick_ticks`/`throttle_ticks` (byte identity). | Normalized floats as the currency (moving the quantization would risk a one-tick rounding difference; Betaflight reads CRSF, so a finer handset has nothing to say). |
| 5 | **No dynamic plugin loading.** `radio_chain`, a private function in `ofs-sim/src/vehicle.rs`, matches on part names; a model is a Rust module, a match arm and a schema enum variant. In-tree. | A runtime registry or dylib/WASM plugins (the parent spec's roadmap item (C)). |
| 6 | **Characterization first**: a committed golden test of today's `ElrsLink` precedes the refactor and must pass unchanged after it. | Refactoring then re-deriving expected values (hides drift). |
| 7 | **Live-flight recording is source-agnostic**: a recorder at `Vehicle::set_sticks`, the funnel for Pilot, joystick and `SetSticks`. | A radio-only recorder (it never needed a serial radio). |
| 8 | **Crossfire and Tracer are not shipped**: they are a profile or a link away once the seams exist, and need sourced numbers. Roadmap rows. | Shipping guessed figures. |

### Out of scope

A physical radio over serial in either direction; telemetry back to a handset; non-CRSF receivers; plugin loading;
a mixer or EdgeTX-style handset behaviour (the `Handset` seam is where it would go); Crossfire/Tracer numbers; M6.

## 3. The three parts and the chain (`crates/ofs-radio`)

One packet slot flows `Handset → Link → Receiver → UART`, with telemetry travelling back.

- **`Handset`** reads the stick and switch signals the bus carries (driven by the joystick, Pilot or `SetSticks`) and
  returns one `TxFrame`: channels as CRSF ticks (AETR order, then AUX1–4, the rest centred) and whether the radio is
  transmitting. It exposes its RF front end: position, antennas and TX power (position and antennas from the world's
  `[handset]`, power from the quad's `[radio]`, where they already live; the builder reads them and fills the link's parameters, so nothing calls
  `Handset::rf_front_end()` yet). The first implementation, `joystick`, is
  today's `sample()` extracted.
- **`Link`** takes the `TxFrame` and the `&Bus` (a link registers the signal handles it reads — the quad's position and
  attitude, the fault signals — at construction; the handset's RF front end is read by the builder) and returns a
  `Delivery`: the channels after latency, or none if the packet was lost; uplink and downlink
  quality (RSSI per antenna, SNR, LQ, the active antenna, link up); and whether a downlink telemetry slot was flown.
  It owns the packet rate and the RNG. Implementations: `elrs` (§4) and `fixed` (§4).
- **`Receiver`** turns a `Delivery` into CRSF bytes on the UART: an RC frame per delivered packet, `LINK_STATISTICS`
  every N delivered packets (as today; `link_stats_interval_packets` is the receiver's cadence, in `ReceiverParams`). It also decodes the flight controller's telemetry frames from the UART and
  queues them (16 frames, oldest dropped when full — today's FIFO). The first implementation is `crsf`.
- **`RadioChain`** is the `Model` (it takes its link's name: `radio.elrs` for `kind = "elrs"`, `radio.fixed` for
  `fixed`, so recordings keep their signal names). Each packet it steps the three parts in order, publishes the link's outputs on the bus
  (`radio.link_up`, `radio.lq`, `radio.rssi`, `radio.snr`, `radio.antenna`, `radio.downlink_lq`), and, when the link
  reports a flown downlink slot, moves one queued frame from the receiver to the handset side's `FcTelemetryHandle`
  (the HUD's source, unchanged).

The hot loop allocates nothing per packet; the dynamic dispatch is three virtual calls at up to 500 packets per second.

## 4. The shipped parts

- `handset = "joystick"`, `receiver = "crsf"` — the extractions above.
- `kind = "elrs"` — today's model with a **band profile table** (frequency in MHz, noise floor, sensitivity per packet
  rate, the CRSF `rf_mode` index per rate). Profiles: `2g4` (today's constants, the default) and `900` (ELRS 900 MHz:
  the frequency feeds `ofs-rf` as it already takes MHz). The `rf_mode` reported in LINK_STATISTICS is the rate's place
  in the band's ascending rate list plus one (0 stays the 4 Hz Crossfire mode), which reproduces today's 2.4 GHz
  50→1, 150→2, 250→3, 500→4. The 900 MHz figures — carrier 915 MHz, LoRa bandwidth 500 kHz (noise floor
  `-174 + 10 log10(500 000) + 6` dBm), downlink power 100 mW, and sensitivities of -123/-120/-117/-112/-111 dBm at
  25/50/100/200/250 Hz — are estimates, the sensitivities read from ExpressLRS's published table
  (https://expresslrs.org/info/signal-health/), and are labelled so.
- `kind = "fixed"` — a geometry-free link: `loss_fraction`, `latency_packets`, optional `burst_on_s`/`burst_off_s`,
  seeded. It is the worked example in "Writing a radio model" and the second, structurally different `Link`.

## 5. Config

The quad file's `[radio]` section keeps every key it has. `kind` names the link (`elrs` or `fixed`); `handset`, `receiver`, `band` and `[radio.fixed]` are new optional keys;
there is no separate `link` key. `handset` and `receiver` are part names (the defaults, `joystick` and `crsf`,
reproduce today's chain), `band` applies to `kind = "elrs"` only (default `2g4`), and the `fixed` link's parameters
live in the `[radio.fixed]` table. `kind = "elrs"` stays and means the default chain. The change is additive, so
`SCHEMA_VERSION` stays 5; a file that uses the new keys fails on an older build through the existing
`deny_unknown_fields`. An unknown part name is a validation error listing the valid names, and keys that belong to
another link (a `band` with `kind = "fixed"`, a `[radio.fixed]` with `kind = "elrs"`) are rejected, not ignored.

## 6. Recording a live flight

An `InputsRecorder` at `Vehicle::set_sticks`. While on, it writes a CSV row whenever any stick value changes; the first
row captures all eight columns at `t_s = 0`. The file is `play_inputs`' format exactly
(`t_s,roll,pitch,yaw,throttle,aux1,aux2,aux3,aux4`); `t_s` is the time since recording started, snapped to the tick the
change applied on; two changes in one tick keep the later; floats use Rust's round-trip format so replay reproduces
them exactly. A change recorded at tick T replays to apply at tick T before that tick's step, so two lockstep replays
of one CSV write byte-identical MCAP (the M4c contract). A replay is not the real-time flight (real-time timing, the
SITL schedule and the link's seeded draws differ); the docs say so. The CSV has no transmitter column, so a link cut
(`K`) is not captured; faults have their own scheduling.

**Protocol 10.** `StartInputsRecording(Empty) → {path}` and `StopInputsRecording(Empty) → {path, rows, duration_s}`; the
server names the file `<logs>/<quad>-inputs-<n>.csv`. The events are `inputs_recording_started` (20) and
`inputs_recording_stopped` (21). Recording stops on `Unload`, a new `Load` and session end (those drop the recorder
without publishing `inputs_recording_stopped`: a client that tracks the state resets it on a new session, as the Godot
game does; a roadmap row); a write failure stops it and is reported once, as the MCAP recorder's failures are. Python:
`sim.record_inputs() -> str` and `sim.stop_inputs_recording() -> str` (the path); the Python and Rust
`PROTOCOL_VERSION` constants go 9 → 10. Godot: the `I` key toggles the recording, and `REC STICKS` shows in the status
line while it runs.

## 7. Testing

1. **Characterization**: `ElrsLink` over several seeds and parameter sets (ideal, fading, obstacles, the fault
   fraction, bursts, latency 0..n, one and two antennas); the UART bytes, output signals and decoded FC telemetry are
   golden. Committed before the refactor; unchanged after.
2. The existing `ofs-radio` and Betaflight suites pass with only mechanical constructor changes; the sandbox quad's
   lockstep MCAP is byte-identical before and after the chain lands.
3. Per part: handset channel order and tick conversion; receiver stats cadence and the drop-oldest telemetry queue;
   the 900 MHz profile (packet-error curve at its sensitivities, an analytic free-space budget); the `fixed` link's
   exact latency, seeded loss statistics and burst timing.
4. **The swap proof**: a custom `Link` (always drop, then always deliver) defined inside a test file builds a chain
   and flies a session through failsafe and recovery, with no core change.
5. Config: old quad files load unchanged; an unknown part name and a mismatched key are errors that name the field.
6. Recording: rows only on change, the first row complete, tick snapping and same-tick coalescing, round-trip floats;
   a recorded CSV replayed twice in lockstep writes identical MCAP; stop on `Unload`; the write-failure path.
7. Performance: the hot loop stays allocation-free; the CI benchmark (1x floor) does not regress.

## 8. Docs and housekeeping

New: "Writing a radio model" (the `fixed` link as the example), `docs/research/serial-radio.md` (the §1 finding, the
usbipd workaround, the untested module-bay route), README and `CHANGELOG` entries, the architecture guide's radio
section. Edited: the phase-2 spec §3 (points here); the old M5 spec's status line (superseded); `docs/roadmap.md` gains
rows for a real serial radio (blocked), Crossfire, Tracer, non-CRSF receivers, and plugin loading, and loses the
carried "real radio over serial" deferral. The Task-1a capture tool and the serial-bridge design are not carried over;
they remain on the `m5-serial-radio` branch, which is left in place until the maintainer says to delete it.

## 9. Task outline (as built: the plan's nine tasks)

1. Characterization goldens for today's `ElrsLink` (per-OS digest files: the libm differs between Windows and Linux).
2. Split the radio into `Handset`/`Link`/`Receiver` and `RadioChain`, wired into the vehicle; goldens identical.
3. Band profiles and ELRS 900 MHz (`band`, `quads/opendrone-5f-900mhz.toml`). 4. The `fixed` link (`[radio.fixed]`,
`quads/opendrone-5f-fixed-link.toml`). 5. The `handset` and `receiver` keys, the swap test and
`docs/writing-a-radio-model.md`. 6. The `InputsRecorder` and the replay determinism test. 7. Protocol 10: the RPCs, the
server and the Python client. 8. The Rust client command and the Godot `I` key. 9. Live checks, the benchmark, the
docs and these amendments.

**Risks.** The refactor must not change seeded output (task 1's goldens). The 900 MHz numbers are estimates and are
labelled so. The Godot toggle needs a hand check on the maintainer's setup.
