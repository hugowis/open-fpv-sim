# M4: sandbox — design

Status: designed 2026-10-11 while M3e executes on its branch; **four scope questions were put to the maintainer and went unanswered (away), so the rulings below marked [ruling-pending] are the designer's best judgment and the first thing to confirm at review.** Parent spec: `2026-10-04-open-fpv-sim-design.md` (§6 Sandbox, §9 milestone M4 — the final v1 milestone). Predecessor: M3e (`2026-10-10-m3e-video-extras-design.md`), still executing; this spec is written against its pinned plan (protocol 7, `quads/opendrone-5f-tramp.toml`, `VtxKind::Tramp`) and **each sub-milestone's implementation plan is written after M3e merges**, against the merged tree (the M3d→M3e precedent). Carried debt it closes: "VTX faults (M4)" (m3a), "The 'VTX interference' fault (M4)" (m3b), the pole-pairs constant and "expose the telemetry slot ratio… if a user misses it" (m3d, the pole-pairs half), "MSP_STATUS failsafe flags are not decoded in Python" (m3c, as a test aid), and the quad file's own "provenance tracking per parameter arrives with the component library (M4)".

## 1. Purpose and scope

M4 makes the simulator a sandbox: components become data files with provenance instead of hand-written estimates, scripts can break the aircraft in the ways real aircraft break, and everything the sim does is recordable and analysable. Exit criteria (§9 row M4): **OpenDrone component library with provenance; v1 fault catalog; MCAP + Rerun; installable Python package.**

**Success looks like:** a user runs `pip install open-fpv-sim[log]`, writes a script that loads a quad assembled from `library/` parts (each parameter tagged `datasheet`/`measured`/`estimated` with a source), injects `MotorFailure(motor=2)` and `BurstLoss(...)` mid-flight, gets Betaflight's failsafe behaviour, stops, and pulls a DataFrame of `imu.*` and `motor.*.omega_radps` out of the MCAP file the session wrote — and two identical lockstep runs produce byte-identical MCAP files. Live, `ofs.rerun.view(sim)` shows the flight while it happens.

### Sub-milestones [ruling-pending: shape]

M3 was delivered as M3a–M3e; M4 follows the same pattern, split along the three pillars plus their shared plumbing. Each sub-milestone gets its own implementation plan against the merged tree of its predecessor; this spec is the contract for all three.

| Sub | Name | Content | Protocol |
|---|---|---|---|
| **M4a** | Component library | `library/` part files, provenance, schema 5, quads as part references, pole pairs from data | unchanged |
| **M4b** | Fault catalog | the seven faults below, model seams, events, python `ofs.faults`, MSP failsafe-flag decode | 8 |
| **M4c** | Logging and packaging | `ofs-log` (MCAP), DataFrame/CSV/Parquet export, Rerun live view, `play_inputs`, PyPI package, blackbox surfacing | 9 |

Order rationale: the library lands first because the fault parameters that want to be data (a prop's imbalance, a motor's pole pairs) then land once, in the part files, instead of being bolted onto the monolithic quad file and migrated weeks later. Faults precede logging so the logging tests can assert fault transients from real sessions.

### Decisions made in the design

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | **Parts by reference only, hard cutover, `schema_version = 5`** [ruling-pending: config cutover]. The three shipped quads (including M3e's Tramp one) are rewritten into `library/` parts; no inline `[frame]`/`[motor]`/… sections survive; no per-parameter overrides in the quad file. | Dual format (inline stays valid; every loader path doubles forever) and optional-parts-with-deprecation (two config dialects through all of v1). Pre-v1 there are no external users to break. |
| 2 | **Provenance is mandatory per parameter.** A part file with a parameter missing from `[provenance]` is a config error naming the field. | Advisory provenance (the milestone's point is that the number's origin is part of the data). |
| 3 | **Faults are runtime bus signals, not config** — the M2 `RadioLinkLoss` pattern generalized: every fault publishes `fault.*` signals; models read them each step; the fault never mutates a model's parameters behind the bus. Injecting and clearing raise events. | Rebuild-the-vehicle faults (slow, un-injectable mid-flight) or per-model flag APIs (N bespoke seams). |
| 4 | **MCAP is written in Rust (`ofs-log`, the `mcap` crate); Rerun is Python-side** [ruling-pending: Rerun]. The live view subscribes the existing state/event streams and publishes through `rerun-sdk` as the optional extra `ofs[rerun]`. | Rerun's Rust SDK inside `ofs-sim` (a very large dependency to carry in the server for a viewer's sake; the 240 Hz state stream is plenty for a live view). MCAP files open in Foxglove unchanged. |
| 5 | **Python distribution name `open-fpv-sim` on PyPI; import name stays `ofs`** [ruling-pending: PyPI name]. `ofs` is taken on PyPI by an unrelated object-storage library (verified 2026-10-11); `open-fpv-sim` is free and matches the project. CI publishes on tag push only. | `ofs-sim`/`ofpsim` (free but less obvious); no publish in M4 (v1 should be announce-ready; a tag-gated job publishes nothing early). |
| 6 | **`play_inputs` is server-side, sample-and-hold, tick-quantized.** The CSV is read and validated once at the RPC; rows apply at the first tick whose time is ≥ the row's `t_s`; `SetSticks` during playback is an `InvalidArgument`. | Client-side replay (a gRPC round-trip per change: slow, and lockstep ordering stops being deterministic). |
| 7 | **Fault state never perturbs a run's randomness:** every added RNG draw happens in a fixed per-step/per-packet position regardless of fault state, and its result is used only when the fault is active; the prop-imbalance baseline is 0 in shipped parts (vibration exists only when a part or fault sets it). So within one build, a fault-inactive run is bit-identical to a no-fault run. New signals (`fault.*`, `motor.*.theta_rad`) re-baseline the cross-build bus digest — that is mechanical, and no physics output changes. | Fault-state-dependent draw counts (activating a fault would perturb every downstream random sequence). |
| 8 | **Crash damage stays out of the catalog.** The master spec's v1 catalog (§6.2) is the contract; collisions already raise events. A crash is composed by the script: detect the collision event, then inject `PropDamage`/`MotorFailure`. | Automatic damage-on-impact (unbounded scope; no damage model exists). |
| 9 | **The weak cell is additive ohmic resistance + OCV offset on one cell; the pack Thevenin model is untouched.** Pack voltage = model voltage − I·ΔR − ΔV while the fault is active; per-cell voltages are synthesized for logging (equal shares, the weak cell carrying the delta). | A per-cell Thevenin refactor (changed numerics for every run to model one fault; capacity fade is honestly recorded as out of scope instead). |

### Out of scope

(C) plugin models and fidelity levels beyond level 1; the repro bundle (§7 of the parent — needs recorded inputs and ring-buffered MCAP, both of which M4 provides; lands post-v1); automatic crash damage; wind/weather faults (no wind model); capacity-fade and self-discharge in the battery; windmilling drag on a dead motor; ESC thermal model (m3a debt, stays); structural transmissibility for vibration (rigid-body transmission only); the telemetry-slot-ratio config (m3d, stays); publishing to crates.io (the Rust side is a repo build, not a library release).

## 2. Component library (M4a)

### 2.1 Layout

```
library/
  frames/    motors/   props/   escs/   batteries/   vtx/
quads/       <quad>.toml + <quad>.betaflight.diff   (as today)
```

One TOML per part, named by its id (`library/motors/opendrone-openmotor-2207.toml` → id `opendrone-openmotor-2207`). v1 ships the part kinds the config actually consumes: **frame, motor, prop, esc, battery, vtx**. The parent's `fcs/`, `receivers/`, `cameras/` wait for models that need them (the FC section is sim plumbing, not hardware; receiver antennas are already quad-file data; there is no camera model) — YAGNI, recorded in carried debt.

### 2.2 Part files

```toml
schema_version = 1
name = "OpenMotor 2207 1950KV"
kind = "motor"

[motor]
kv_rpm_per_v = 1950.0
resistance_ohm = 0.07
no_load_current_a = 1.2
rotor_inertia_kgm2 = 6.0e-6
pole_pairs = 7

[provenance]
[provenance."motor.kv_rpm_per_v"]
source = "estimated"        # datasheet | measured | estimated
ref = ""                    # optional URL / doc
note = "M1 estimate"
```

Rules: every parameter of the part's one data section (`[motor]`, `[frame]`, …) appears in `[provenance]` (validation error otherwise, naming file and field); unknown `schema_version` fails explicitly; units stay in key suffixes; validation collects all errors before failing (the ofs-config contract). **Pole pairs land here** — `ofs-fc::sitl::esc_telemetry` reads the motor part's `pole_pairs` instead of the `POLE_PAIRS` constant, closing the m3d debt (a diff with `motor_poles ≠ 14` stops silently scaling the RPM filter). The prop part gains `imbalance_kgm` (default 0; the vibration model's amplitude input, M4b). The vtx part carries the protocol kind (SmartAudio/Tramp), power tables and the antenna; the quad keeps band/channel/uart (per-aircraft wiring, not part data).

### 2.3 Quads

```toml
schema_version = 5
name = "OpenDrone 5F freestyle"
frame = "opendrone-5f"                  # motor positions, spin, contact, mass, drag
motors = ["opendrone-openmotor-2207"]   # list: mixed builds; 1 entry broadcasts to all
props   = ["opendrone-5in-prop"]
escs    = ["opendrone-4in1-esc"]
battery = "opendrone-6s-1300"
vtx     = "opendrone-vtx-sa-1w"         # optional, as today
[sim] [home] [initial] [fc] [radio] [esc_telemetry] [osd]   # unchanged, quad-level
```

Part ids resolve against **`library/` beside the quads directory** (`<quad dir>/../library`), overridable with `OFS_LIBRARY` (one path, v1). Unknown id → config error listing the searched directory. Parts do not reference parts (no composition graph in v1); the diff file, EEPROM directories and `force_battery_cell_count` validation are unchanged and quad-level. The three shipped quads are migrated with values unchanged, all provenance `estimated` — the point is the shape and the honesty, not new numbers; datasheet values are community follow-up.

## 3. Fault catalog (M4b)

The v1 catalog is the parent's §6.2 list. Every fault: a python dataclass in `ofs.faults` → proto message in the `Fault` oneof → `Fault` enum variant in `vehicle.rs` → `fault.*` bus signals → read by its model. `ClearFaults` clears all. Each fault's mechanism and its observable:

| Fault (python) | Mechanism | Observable |
|---|---|---|
| `RadioLinkLoss()` | exists today (M2) | uplink silent, Betaflight failsafes |
| `RadioLoss(fraction)` | extra per-packet Bernoulli loss, drawn every packet in a fixed stream position, used only when active | LQ drops by ≈ fraction; failsafe at high fractions |
| `BurstLoss(on_ms, off_ms)` | deterministic periodic window (phase from sim time) losing uplink packets like `RadioLinkLoss` | LQ sawtooths; a burst longer than Betaflight's failsafe timeout failsafes |
| `MotorFailure(motor, scale)` | `fault.motor.<i>.scale` in [0,1]: EscMotor scales torque and current; `scale=0` is a dead motor — omega decays through prop drag, no windmill drag (debt) | the motor's current and RPM fall; the quad yaws/tumbles; Betaflight compensates until it can't |
| `PropDamage(prop, thrust_scale, imbalance_kgm)` | scales that propeller's CT and CP; sets its imbalance for the vibration model (below) | hover needs more throttle; `imu.gyro_frd_radps` grows a line at the motor's rotation frequency |
| `WeakCell(cell, r_scale, v_offset)` | pack voltage − I·(Δr) − Δv while active (decision 9); publishes synthesized `battery.cell_voltages` | sag deepens under load; Betaflight's voltage reads it |
| `GyroFault(bias_radps, noise_scale)` | adds to the IMU's bias, multiplies its noise std (≥0) | `imu.gyro_frd_radps` shifts by the vector; Betaflight's attitude drifts against truth |
| `VtxInterference(freq_mhz, power_mw, position_ned_m)` | the video link appends one runtime emitter to its interference computation each field step (the same math as world emitters) | `video.interference_dbm` rises, SNR falls, the picture degrades |

**Motor phase and the vibration model.** `EscMotor` publishes `motor.<i>.theta_rad` (phase, integrated from omega, wrapped — pure integration, no RNG). The IMU, when any motor's imbalance > 0, adds per-motor rotating disturbance: force `m·r·ω²` at the motor position and its arm torque, rigid-body-projected onto the IMU (the parent §4.2 always intended "motor-induced vibration from imbalance"; transmissibility is out of scope and the imbalance constants are estimates recorded in `docs/research/faults.md`). Baseline imbalance 0 keeps no-fault runs bit-identical (decision 7).

**Events and decoding.** `fault_injected`/`fault_cleared` events carry the fault's name. `MSP_STATUS` failsafe flags are decoded in the python client (m3c debt) so the `BurstLoss`/`RadioLoss` tests assert *that* Betaflight fails safed, not only the symptoms.

## 4. Logging: `ofs-log` and MCAP (M4c)

New crate `ofs-log` (the parent §4.2's missing row). A **`Recorder`** is a scheduler model, appended last, that only **reads** the bus (recording never changes a run: the digest is identical with and without it):

- **Spec:** a list of `(glob, rate_hz)` resolved against bus signal names at build — e.g. `("imu.*", 1000)`, `("motor.*", 240)`. A **default table** covers every signal class (body 240 Hz, imu 1 kHz, motor/esc 240 Hz, battery 10 Hz, radio/video/rc/fault 50 Hz, catch-all 50 Hz); callers override per pattern. Unmatched patterns are an error listing near-misses.
- **Writer:** the `mcap` crate; one channel per signal (name = bus name); records are JSON (`{"v": x}` / `[x,y,z]` / `[w,x,y,z]` for vec3/quat); `publish_time = log_time` = simulated time in ns. **Determinism:** the writer pins all metadata (fixed library string, no wall-clock anywhere); two lockstep runs with the same seed and inputs produce **byte-identical files** — the §8.4 determinism test upgrades from bus digest to MCAP file hash (the digest test stays as the faster unit).
- **API:** `StartRecording(signals, …)` / `StopRecording()` → summary `{path, messages, first_s, last_s}`. Files land under `<data_dir>/logs/<quad-stem>-<n>.mcap`; the server names them (client-supplied paths are refused — no server-side arbitrary writes).
- **Python:** `sim.recording.start(patterns=None)` / `.stop()`; `ofs.reader.to_dataframe(path, patterns)` with pandas (extra `ofs[log]`, with `mcap`), plus CSV (stdlib) and Parquet (`df.to_parquet`) export helpers.
- **Blackbox surfacing:** on `Unload`, the session copies SITL's `LOG*.BFL` from the firmware dir into `<data_dir>/logs/` (created as needed; `sim.blackbox_files()` lists them) — the parent §6.3's "Blackbox for Blackbox Explorer" stops being a buried directory.

**Live view (Rerun, Python-side — decision 4):** `ofs.rerun.view(sim, rate_hz=30)` (extra `ofs[rerun]`, `rerun-sdk`) streams states/events to the Rerun viewer: 3D pose, battery/motor/link curves, events as text. Documented limitation: state-stream rates (≤ 240 Hz), not full bus rates; researchers wanting 8 kHz pull the MCAP.

## 5. Scripting: `play_inputs`

`PlayInputs(path)` / `StopInputs()`; CSV with header `t_s,roll,pitch,yaw,throttle,aux1..aux4` (any subset; monotonic `t_s`, validated once with line numbers). Sample-and-hold at tick boundaries (decision 6). Works in lockstep and realtime; `inputs_started`/`inputs_ended` events; `SetSticks` during playback is an `InvalidArgument` (loud beats silently-lost). This is also the enabling half of the deferred repro bundle.

## 6. Packaging

- **`python/pyproject.toml`:** name `open-fpv-sim`, import `ofs` (decision 5). Extras: `[log]` = `mcap`, `pandas`; `[rerun]` = `rerun-sdk`; `[dev]` unchanged plus these.
- **CI:** the existing python jobs grow a wheel build; a **publish job runs only on tag push** (`v*`) via PyPI trusted publishing. Nothing publishes from merges.
- **Docs:** README's install section shows `pip install open-fpv-sim[log,rerun]` and the sandbox example (the parent §6.2 script, which works verbatim after M4).

## 7. Protocol and clients

- **M4b → protocol 8** (M3e ships 7): the `Fault` oneof grows its messages; `Fault`, `InjectFault` unchanged in shape; `EventKind` grows `fault_injected`, `fault_cleared`. Python `ofs.faults` grows the dataclasses; `ofs-client` maps none of them (the Godot client injects no faults).
- **M4c → protocol 9:** `StartRecording`/`StopRecording`/`PlayInputs`/`StopInputs` RPCs, `RecordingSignal`/`RecordingSummary`/`InputsFile` messages; `EventKind` grows `inputs_started`, `inputs_ended`, `recording_started`, `recording_stopped`.
- All clients move together on each bump (the M3 rule); M4a changes no wire bytes.

## 8. Order of work, failure handling and tests

**Within each sub-milestone, riskiest first** (the M3e rule):

- **M4a:** the ofs-config schema/loader split and part resolution (all validation paths) → the shipped quads' migration → pole pairs through esc_telemetry → docs (`README`, `dev-setup.md`, a `docs/research/component-library.md` recording sources for every shipped part parameter).
- **M4b:** the battery/ESC/IMU/ELRS/video seams first (`fault.*` signals, `theta_rad`, the additive weak-cell term, the runtime emitter), each with its no-fault-is-bit-identical lockstep test → the `Fault` plumbing (proto, enum, events, python) → the faults one by one, each with its live-SITL test → `docs/research/faults.md`.
- **M4c:** `ofs-log` + the deterministic-writer test (riskiest: byte-identity) → RPCs + python recording/reader → Rerun view → `play_inputs` → packaging/CI/README.

**Failure handling** (the §7 principle: drone failures are behaviour, simulator failures are loud): a recording disk error stops the recording with an event, the session flies on; a malformed parts/CSV/inputs file is a config error at load/RPC time (collected, with file and line); an unknown fault field or out-of-range value is `InvalidArgument`; a fault on a quad without the matching hardware (e.g. `VtxInterference` with no VTX) is `InvalidArgument`, not a no-op.

**Tests:** unit — provenance validation (missing field, unknown schema, unknown part id with directory listed), per-fault mechanism math (weak-cell sag, thrust scaling, bias add), MCAP byte-identity, CSV validation; simulator (lockstep) — every fault's observable on the open-loop quad, no-fault runs bit-identical to the M3e baseline, fault-inactive runs identical (decision 7); live SITL — `MotorFailure` (Betaflight holds, then doesn't), `BurstLoss` → failsafe **flag** decoded, `WeakCell` → Betaflight's OSD voltage, `VtxInterference` → picture metrics; python — the parent §6.2 example script verbatim as an integration test; packaging — a clean-venv `pip install` of the built wheel runs it.

## 9. Risks

| Risk | Mitigation |
|---|---|
| The `mcap` crate embeds nondeterministic bytes (chunk offsets, CRCs, library string), breaking byte-identity. | Pinned metadata and a fixed write order; the byte-identity test is written **first** in M4c against a throwaway two-signal recording; fallback of last resort is hashing the message records only, recorded as debt if taken. |
| Per-fault seams touch every model crate at once (M4b is wide, not deep). | One seam per model with its own bit-identity test before any fault lands; the faults themselves are then independent tasks. |
| M3e merge conflicts (vehicle.rs, ofs-config, quads/, proto). | M4a is written after M3e merges (the M3d→M3e rule); the overlap regions are disjoint (Handles/Fault vs VideoHandles; quad parts vs VtxKind/world). |
| Provenance-mandatory makes quick local experiments tedious. | `OFS_LIBRARY` points at a user dir; a one-screen example part ships; the strictness is the milestone's deliverable. |
| `open-fpv-sim` is squatted before M4c publishes. | The publish job exists from M4c's first CI change; the first tag that will ever publish is the maintainer's call. |
| Rerun SDK churn (pre-1.0 APIs). | The extra is optional and the module is thin (state stream → entities); MCAP is the durable artefact, Foxglove the fallback viewer. |
| The vibration model's estimates are wild guesses. | Imbalance defaults to 0 (opt-in); constants carry provenance in `docs/research/faults.md`; community measurement is the validation-report path (§8.5 of the parent). |

## 10. Carried-debt ledger

Closes: m3a "VTX faults", m3b "The 'VTX interference' fault", m3d pole pairs, m3c "MSP_STATUS failsafe flags", the quad file's provenance header. Opens (recorded at merge): fcs/receivers/cameras part kinds absent (no models need them); windmilling drag; battery capacity fade; vibration transmissibility; repro bundle (post-v1, enabled by M4c); MCAP-in-Foxglove live tailing untested.
