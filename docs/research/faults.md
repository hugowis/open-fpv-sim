# The fault catalog

How the simulator breaks the aircraft: eight faults, injectable from Python (`ofs.faults`) or gRPC, each one
written once by `Vehicle::set_fault` as `fault.*` bus signals (or, for the one non-scalar fault, a shared handle)
and read by its owning model every step. Injecting and clearing raise `fault_injected` / `fault_cleared` events;
`clear_faults()` returns every signal to its identity, and the prop imbalance to its part data. A fault the quad
cannot carry (`MotorFailure(motor=7)` on a four-motor quad, `VtxInterference` without a `[vtx]`) is refused at
inject with `InvalidArgument`, never a no-op. The design is
`docs/superpowers/specs/2026-10-11-m4-sandbox-design.md` §3; the wire shapes are protocol 8
(`proto/ofs/v1/sim.proto`), the Python dataclasses live in `python/ofs/faults.py`, and the underlying link and
video models are the ones `docs/research/elrs-link.md` and `docs/research/video-link.md` describe.

A fault's own parameters are the scenario, not physics — a `fraction` or an `on_ms` is what the script asked to
happen, so they carry no provenance beyond being user input. The model constants that turn a parameter into
behaviour do carry provenance, and the standing rule from the m3c and m3b notes applies to all of them:
**every constant is an estimate or a user input; none is measured.**

## Determinism (spec decision 7, refined)

The catalog adds exactly one source of simulator randomness: `RadioLoss`'s per-packet Bernoulli, drawn from a
**dedicated fault RNG stream** in the ELRS link (`model_rng(seed, "elrs-faults")`, one ChaCha8 stream beside the
model's own). The draw sits in a fixed per-packet position whatever the fault state, so even *fault-active* runs
differ from fault-free ones only in the loss decisions themselves — the model's own stream (fades, uplink and
downlink error draws) never shifts. `BurstLoss` is a pure function of simulated time and draws nothing; the IMU's
vibration and the gyro fault are deterministic; the pack and video faults draw nothing at all. Multiplicative
fault signals start at their identity (1.0, written once at vehicle build), additive ones at the bus's zero, so a
fault-inactive run is bit-identical to a no-fault run.

## The radio faults

All three fold into the ELRS receiver's one receive decision
(`crates/ofs-radio/src/elrs.rs`):

    received = transmitting && draw >= packet_error_rate(rssi, sensitivity)
                            && fault_draw >= fraction && !in_burst

- **`RadioLinkLoss`** — the M2 original the catalog grew around. While active the receiver's transmitter gate is
  false: every uplink packet is lost, the receiver goes silent as ExpressLRS does by default, and Betaflight's own
  failsafe takes over. No constants — it is a switch.
- **`RadioLoss(fraction)`** — one extra Bernoulli per uplink packet from the fault stream: a packet is lost when
  `fault_draw < fraction`, on top of the link model's own packet error rate. `fraction = 0.0` is a legal no-op
  (bit-identical digests); `fraction = 1.0` loses every packet with the same timing as `RadioLinkLoss`.
  The downlink is untouched (see the carried debt).
- **`BurstLoss(on_ms, off_ms)`** — the uplink loses every packet while
  `in_burst = on_s > 0 && time_s % (on_s + off_s) < on_s`: a deterministic square wave anchored at session start,
  no randomness anywhere. An on-window longer than Betaflight's failsafe timeout drives a real failsafe (stage 1
  then stage 2); the off-window delivers again and Betaflight recovers. The live test drives exactly that and
  asserts the flags below.

| Fault | Constant | Value | Provenance |
|---|---|---|---|
| `RadioLoss` | `fraction` | per-packet loss probability, [0, 1] | user input (the scenario) |
| `BurstLoss` | `on_ms`, `off_ms` | the window's two legs, seconds on the bus | user input |

## Motor failure

`MotorFailure(motor, scale)` acts inside the ESC/motor model (`crates/ofs-electrical/src/esc_motor.rs`): the phase
current is scaled *after* the ESC's clamp, so a half-scale motor draws half the clamp current and makes half the
torque while current-limited — the transient where the failure is visible:

    I = ((duty·V_bus − Ke·ω) / R).clamp(±limit) × scale
    net torque = Ke·I − friction − load

The published `motor.{i}.current_a` and `esc.{i}.bus_current_a` (`duty·I`) carry the scaled value, so Betaflight's
ESC telemetry reads the failure too. A dead motor (`scale = 0`) draws no current and coasts down through prop drag
— the load torque — instead of stopping dead; there is no windmilling model (carried debt). The rotor phase
`motor.{i}.theta_rad` integrates whatever ω remains (`θ += ω·dt`, wrapped to [0, 2π)) and freezes when the rotor
stops, which is the physically right input to the vibration model.

| Constant | Value | Provenance |
|---|---|---|
| `scale` | torque/current multiplier, [0, 1] | user input |
| Ke, R, current limit, no-load current | the motor and ESC parts' own | estimated (`docs/research/component-library.md`) |

## Prop damage and the vibration model

`PropDamage(prop, thrust_scale, imbalance_kgm)` does two things (`crates/ofs-physics/src/propeller.rs`):

- **Thrust loss.** One `fault.prop.{i}.thrust_scale` signal multiplies both outputs of the propeller's CT/CP
  tables — thrust and torque together, applied in `Propeller::step` after the lookup. Clearing the fault restores
  the signal to 1.0.
- **Imbalance.** The fault writes `prop.{i}.imbalance_kgm` — the same signal the prop part's data initializes at
  build (the shipped parts carry 0.0). Clearing restores the part baseline, so part-level and fault-level
  imbalance are indistinguishable at the IMU, and need not be.

The vibration model (`crates/ofs-sensors/src/imu.rs`) turns imbalance into what a real pilot sees in the goggles —
jello, and a gyro buzzing at the motor frequency. For each motor with `imbalance > 0` and `ω > 0`, a rotating
force in the prop plane (body FRD x/y):

    F = imbalance_kgm · ω²    at phase θ_i + α_i

where `θ_i` is the motor's integrated rotor phase and `α_i = atan2(motor_y, motor_x)` its azimuth. The IMU adds
`F/mass` to its specific-force axes, and a rate disturbance on the roll/pitch axes whose amplitude is the rotating
moment's angular rate, `|r|·F / (I·ω)` — `r` the motor position's lever arm, `I` the matching frame inertia
component; the moment leads the force by a quarter turn (M ⊥ F). Both disturbances rotate at the motor's rotation
frequency. The model is deterministic (phase-integrated, no RNG), contributes exactly zero when every imbalance is
zero (gated, so the all-quiet path stays bit-identical), and a stopped motor contributes exactly zero (no 0/0).

This is **rigid-body transmission, below-resonance amplitude only**: the airframe responds as a rigid body, no
structural model, no transmissibility — the m3b/m3c debt phrasing applies verbatim: **every constant is an
estimate; none is measured** (the mass and inertia come from the frame part, whose provenance ledger records
them as estimates). Real props fail with a spectrum, not a pure tone; the input `imbalance_kgm` is a scenario
figure, and community measurement is the validation path recorded in the M4 spec.

| Constant | Value | Provenance |
|---|---|---|
| `thrust_scale` | thrust/torque multiplier, ≥ 0 | user input |
| `imbalance_kgm` | rotating imbalance (m·r), ≥ 0 | user input (part default 0, itself an estimate) |
| `mass_kg`, `inertia_kgm2` | the frame part's | estimated |

## Weak cell

`WeakCell(cell, r_offset_ohm, v_offset_v)` is additive on the pack Thevenin, per spec decision 9 — the battery
model's own state is untouched (`crates/ofs-electrical/src/battery.rs`):

    voltage = OCV(soc) − I·(r0 + Δr) − v1 − Δv

The droop deepens by exactly `I·Δr + Δv` under load; SoC, current and the `v1` polarization state keep the
untouched math, so the fault is invisible the instant the load is off — like a real weak cell. Cell voltages are
**synthesized** for the state stream (protocol 8's `State.battery_cell_voltages`): every cell gets the published
pack voltage divided equally, and the weak cell additionally carries `−Δv − I·Δr`, its own share of the additive
terms. The equal share is a modelling simplification — a real pack's cells differ — and Betaflight only ever sees
the pack voltage anyway (the shipped diff leaves `report_cell_voltage` off; see `docs/research/elrs-link.md`).

| Constant | Value | Provenance |
|---|---|---|
| `r_offset_ohm`, `v_offset_v` | extra series resistance / voltage drop, ≥ 0 | user input |
| `fault.battery.weak_cell` | carries cell index + 1 (0 = none) | a ruling, see the carried debt |

## Gyro fault

`GyroFault(bias_radps, noise_scale)` is two terms on the IMU's gyro line
(`crates/ofs-sensors/src/imu.rs`):

    gyro = body rate + configured bias + fault bias + noise · fault_noise_scale

The accelerometer is untouched — a real gyro fault does not move the accelerometers, and the flight controller's
overnight bias calibration is Betaflight's business, not the simulator's. `noise_scale` multiplies the gyro noise
std exactly (the same seed produces the same sample sequence, scaled), so a scale of 3 is a three-times-noisier
gyro and a scale of 0 a suspiciously clean one.

| Constant | Value | Provenance |
|---|---|---|
| `bias_radps` | added bias, rad/s, body FRD | user input |
| `noise_scale` | multiplier on the gyro noise std, ≥ 0 | user input |

## VTX interference

`VtxInterference(freq_mhz, power_mw, position_ned_m)` puts a runtime transmitter into the analog video link's
interference sum (`crates/ofs-video/src/link.rs`). It rides the shared-handle pattern (the OSD handle's): the
vehicle stores an `Option<Emitter>`, and the link appends it to the world's `[[emitters]]` before the same
propagation — path loss, antenna patterns, the adjacent-channel rejection of `docs/research/video-link.md`'s
Interference section — as the world's own emitters get. The per-field interference cache is keyed on
(frequency, injected emitter), so an injection or a clear takes effect on the next field even though the
frequency is unchanged.

| Constant | Value | Provenance |
|---|---|---|
| `freq_mhz`, `power_mw`, `position_ned_m` | positive frequency and power, finite position | user input |
| the emitter's antenna | 2 dBi RHCP omni, axis down | **estimated** — the same default as the shipped VTX antennas (`library/vtx/*.toml`), standing in for whatever is actually radiating |

## What Betaflight shows: the failsafe decode (m3c debt closed)

The radio faults' deepest observable is Betaflight failing safe for the right reason. The decoded observable is
`MSP_STATUS`'s **arming-disable flags** — **`RX_FAILSAFE` (1 << 2)** from failsafe stage 1 on, **`FAILSAFE`
(1 << 1)** once stage 2 engages — and explicitly *not* the `BOXFAILSAFE` pilot-switch mode bits: a real RX-loss
failsafe never sets them (`failsafe.c` only reads `IS_RC_MODE_ACTIVE(BOXFAILSAFE)`). The walk of the MSP_STATUS
payload is pinned, byte-provenanced, in `python/tests/msp_status.py`'s docstring (Betaflight 2026.6.2:
`runtime_config.h:44-45`; set at `failsafe.c:195/214/231` and `:490`, cleared on recovery at `:204/:521`), and the
live burst test asserts both flags during the burst and the `FAILSAFE` bit's clear after recovery.

## Where it shows

- Bus signals: the `fault.*` seams above, `prop.{i}.imbalance_kgm` and `motor.{i}.theta_rad` (state, read by the
  vibration model).
- The state stream: `State.battery_cell_voltages` (protocol 8); the radio faults show through `State.radio`
  (LQ, `link_up`), the video fault through `State.video` (SNR, `interference_dbm`), the pack fault through
  `State.battery_voltage_v` and Betaflight's own telemetry.
- Events: `fault_injected` / `fault_cleared` with the fault's name (deterministic order, the spec table's);
  the radio faults raise `LINK_DOWN` / `LINK_UP` through the link's own edge detector.
- The game: the HUD renders the fault events among its toasts; the **K** key injects and clears exactly this
  fault (`RadioLinkLoss`).
- Python: `ofs.faults.{RadioLinkLoss, RadioLoss, BurstLoss, MotorFailure, PropDamage, WeakCell, GyroFault,
  VtxInterference}`, `sim.inject(fault)` / `sim.clear_faults()`.
