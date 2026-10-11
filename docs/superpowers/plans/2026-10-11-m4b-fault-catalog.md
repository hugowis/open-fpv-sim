# M4b: fault catalog — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The v1 fault catalog works end to end — seven new faults (radio loss, burst loss, motor failure, prop damage, weak cell, gyro fault, VTX interference) injectable from Python, each observable on the bus and in Betaflight's own behaviour, with fault-inactive runs bit-identical to fault-free ones.

**Architecture:** The M2 `RadioLinkLoss` pattern generalized: every fault is one or more `fault.*` bus signals (or a shared handle for the one non-scalar fault), written once by `Vehicle::set_fault`, read by the owning model each step. Model seams land FIRST with identity defaults so no-fault runs are bit-identical; the plumbing (protocol 8, events, Python dataclasses) lands second; then the faults themselves, one seam-consumer pair at a time.

**Tech Stack:** Rust workspace (ofs-core, ofs-electrical, ofs-physics, ofs-sensors, ofs-radio, ofs-video, ofs-fc, ofs-sim, ofs-proto, ofs-client), Python client (grpcio; `ofs.faults`), Betaflight SITL (pinned 2026.6.2, unchanged), pytest live suite (WSL).

**Spec:** `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md` §3 (fault catalog), §1 decisions 3, 7, 9, §7 (protocol 8), §8's M4b row — the plan argues from the spec; executors read both.

## Global Constraints

- **Protocol 8.** `PROTOCOL_VERSION` goes 7 → 8 in `crates/ofs-proto/src/lib.rs` AND `python/ofs/client.py:22`; the `sim.proto` header comment says so. The Rust client (`ofs-client`) reads `ofs_proto::PROTOCOL_VERSION` and moves automatically. The proto wire changes are exactly: the `Fault` oneof grows 7 messages, `EventKind` grows 2 values, the `State` message gains `battery_cell_voltages`. Nothing else on the wire.
- **Determinism.** The only new simulator randomness is `RadioLoss`'s per-packet Bernoulli, drawn from a **dedicated fault RNG stream** in `ElrsLink` (`model_rng(seed, "elrs-faults")`) — the model's own stream is untouched, so fault-inactive runs are bit-identical to the pre-M4b tree (stronger than spec decision 7's within-build requirement; see Rulings). `BurstLoss` is a pure function of simulated time. The IMU vibration model is deterministic (phase-integrated). No other fault draws randomness. New bus signals (`fault.*`, `motor.*.theta_rad`) re-baseline the cross-build bus digest — mechanical, no physics output changes.
- **Identity defaults.** Multiplicative fault signals start at their identity (`scale`/`thrust_scale`/`noise_scale` = 1.0, written once by `Vehicle::build` after `set_transmitter`, the `set_sticks` pattern); additive fault signals start at 0 (the bus default). `prop.{i}.imbalance_kgm` is initialized from the part data (the vehicle stores the baseline; `PropDamage` cleared restores it).
- **Value preservation for seams.** Tasks 1–2 change no behaviour: every existing test passes unchanged, the shipped quad's physics outputs are identical, and the only digest deltas are the new signals' own entries.
- **Rust style.** Never run bare `rustfmt`; hand-indent new code. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` after every task that lands Rust — CI's first gate is clippy.
- **Python file edits** (stubs and test helpers) open files in binary mode and write LF.
- **Live SITL tests** run inside WSL via `uv run --with grpcio,protobuf,pytest` + a Linux-built `ofs-sim`, `PYTHONPATH=python` (docs/dev-setup.md option 2). Full python suite after Tasks 3 and 7.
- **Regenerating stubs:** `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto` (dev-setup.md's recipe; requires `pip install grpcio-tools` or the dev extra).
- **Commits:** one per task, conventional style (`feat(radio): ...`, `feat(proto)!: protocol 8 ...`), after the task's tests pass.
- **Drone failures are behaviour, simulator failures are loud** (spec §7): a fault a quad cannot carry (no VTX, bad index) is `InvalidArgument` at inject, never a no-op.

## Rulings and facts verified in advance

Verified against the merged M4a tree (`ed4a289`, protocol 7) and the pinned Betaflight checkout (`~/ofs/betaflight` @ 2026.6.2, e0b7bb01b17b) while the plan was written. They are the contract where the spec is silent.

1. **Dedicated fault RNG stream (refines spec decision 7).** `ElrsLink` gains a second `ChaCha8Rng` from `model_rng(seed, "elrs-faults")`; `RadioLoss`'s draw happens once per packet step in a fixed position (immediately after the existing uplink loss draw) but consumes only from the fault stream. The model's own stream — fades, uplink/downlink loss draws — stays byte-identical to pre-M4b, so even *fault-active* runs differ from their fault-free counterparts only in the loss decisions themselves.
2. **Identity-default signals** (the vehicle writes them once at build): `fault.motor.{i}.scale = 1.0`, `fault.prop.{i}.thrust_scale = 1.0`, `fault.imu.noise_scale = 1.0`. Everything else is bus-zero (= no fault): `fault.radio.loss_fraction`, `fault.radio.burst_on_s`, `fault.radio.burst_off_s`, `fault.battery.r_offset_ohm`, `fault.battery.v_offset_v`, `fault.battery.weak_cell`, `fault.imu.gyro_bias_radps`.
3. **Prop imbalance is one signal per motor** — `prop.{i}.imbalance_kgm` (no `fault.` prefix: it is state, initialized from `cfg.props[i].imbalance_kgm` at build; `Vehicle` stores `prop_imbalance_baseline: Vec<f64>`; `PropDamage` writes the fault's value, its clear restores the baseline). The IMU reads this one signal — part-level and fault-level imbalance cannot be distinguished at the IMU, and need not be.
4. **`VtxInterference` rides a shared handle, not bus signals** (the `OsdHandle`/`FcTelemetryHandle` pattern — an emitter is a 4-field struct). `crates/ofs-video/src/link.rs` gains `VtxInterferenceHandle` (`Arc<Mutex<Option<Emitter>>>`-shaped, `Default` = no emitter); `VideoLink::new` takes it; `interference_for` appends it to `params.world.emitters` when `Some`. `set_fault` stores `Some`, `clear_faults` stores `None`. The handle is created in `vehicle.rs::build` and stored on `Vehicle`.
5. **BurstLoss window math:** `in_burst = on_s > 0.0 && (time_s % (on_s + off_s)) < on_s` — deterministic, phase anchored at session start (`time_s` = 0). Validation: `on_ms > 0`, `off_ms > 0` (a permanent loss is `RadioLinkLoss`, not a degenerate burst).
6. **Betaflight failsafe decode (pinned against the checkout):** real RX-loss failsafe never touches the pilot-switch path (`IS_RC_MODE_ACTIVE(BOXFAILSAFE)` is false — `failsafe.c` only reads it), so the decoded observable is `MSP_STATUS`'s `armingDisableFlags`: **`ARMING_DISABLED_RX_FAILSAFE = 1 << 2`** (set at `failsafe.c:195/214/231` from stage 1 on) and **`ARMING_DISABLED_FAILSAFE = 1 << 1`** (set at `failsafe.c:490` when stage 2 engages, unset at `:521` on recovery) — `runtime_config.h:42-44`. The MSP_STATUS payload walk (from `msp.c` `case MSP_STATUS:`, command 101): `[0..2]` u16 cycle time, `[2..4]` u16 I2C errors, `[4..6]` u16 sensor bitmap, `[6..10]` u32 LE flight-mode flags, `[10]` u8 pid profile, `[11..13]` u16 system load, `[13..15]` u16 gyro cycle time, `[15]` u8 extended-box byte count, that many bytes, `[·]` u8 arming-disable flag count, then u32 LE `armingDisableFlags`. The helper walks the variable-length middle; no MSP_BOXIDS request is needed. (For the record: `BOXFAILSAFE`'s MSP permanentId is 27, `msp_box.c:76` — not needed by this decode.)
7. **`WeakCell` synthesis formula (spec decision 9):** the battery model subtracts the fault's `Δr` and `Δv` from the published pack voltage only (`voltage = ocv − I·(r0 + Δr) − v1 − Δv`); SoC, current and the v1 polarization state use the untouched Thevenin math. `fault.battery.weak_cell` carries **cell index + 1** (0 = no weak cell), so `Vehicle::state()` can synthesize `battery_cell_voltages`: every cell gets `published_voltage / cells`, and the weak cell additionally carries `−Δv − I·Δr` (its own share of the additive terms). Inject validation: `cell < battery_cells` — the vehicle stores `battery_cells: u32` at build.
8. **Vibration model (spec §3, rigid-body transmission only):** for each motor with `imbalance > 0`, a rotating force in the prop plane (body FRD x/y): `F = imbalance_kgm · ω²` at angle `θ_i + α_i`, where `θ_i` is the motor's `theta_rad` and `α_i` the motor's azimuth from `atan2(motor_y, motor_x)`. The IMU adds `F/mass` to its specific-force axes and a rate disturbance `|r × F| / (I·ω)` on the roll/pitch axes (the rotating moment's angular rate amplitude, `r` = the motor position's lever arm, `I` = the matching inertia component) — both at the motor's rotation frequency. Below-resonance rigid-body transmission only; transmissibility stays out of scope (m4b carried debt), and every constant is an estimate recorded in `docs/research/faults.md`. With imbalance = 0 everywhere the model contributes exactly zero.
9. **EscMotor failure scaling:** `current = ((duty·Vbus − ke·ω)/R).clamp(±limit) · scale`; `net_torque = ke·current − friction − load`; the published `motor.{i}.current_a` and `esc.{i}.bus_current_a` carry the scaled values (a dead motor draws no current). `theta_rad` integrates regardless of scale (`θ += ω·dt`, wrapped to `[0, 2π)`) — a stopped motor has a frozen phase, which is the physically right vibration input.
10. **`PropDamage` scales thrust and torque together** (spec: "scales that propeller's CT and CP") — one `thrust_scale` signal multiplies both outputs of `Propeller::thrust_torque`, applied in `Propeller::step` after the lookup. Validation: `thrust_scale ≥ 0`, `imbalance_kgm ≥ 0`.
11. **Fault events ride the session's edge detector** (the collision/link pattern): `Vehicle` gains `active_fault_names() -> Vec<String>` (deterministic order: the spec table's order, `["motor_failure", ...]` style, matching the python class names in snake case); `Session` stores `last_faults: Vec<String>` and `events_for` raises `fault_injected`/`fault_cleared` with the fault name in the message. No `State` message change for fault names.
12. **Client event kinds:** `ofs-client/src/model.rs`'s `EventKind` enum gains `FaultInjected`/`FaultCleared` (the `from_pb` match and `as_str`); the Godot HUD shows them via its existing event rendering. Python's `_kind_name` is pb-driven and needs nothing.
13. **CI's sitl job names its live tests explicitly** (`.github/workflows/ci.yml`'s pytest invocation); Task 7 adds `test_sitl_faults.py` to that list. The pre-existing `test_sitl_link.py` omission (m3e carried debt) stays — not this milestone's debt to close.

## Review Focus

The five inputs the spec implies but no task's tests fully exercise, most likely first — each is pinned by the named test:

1. **A fault injected on hardware the quad lacks** (`VtxInterference` on a quad without `[vtx]`, `MotorFailure(motor=7)`): `InvalidArgument` naming the fault and the reason, the sim keeps running, no partial state — `an_unknown_motor_index_is_invalid_argument`, `vtx_interference_without_a_vtx_is_invalid_argument` (Task 3).
2. **Fault-active-then-cleared restores the exact prior state**: after `clear_faults`, the signals are back at identity (including `prop.{i}.imbalance_kgm` restored from the part baseline) and the digest of a run-past-clear equals the digest of a never-faulted run from the same point — `clearing_prop_damage_restores_the_part_imbalance`, `a_cleared_run_rejoins_the_fault_free_digest` (Tasks 5/6).
3. **RadioLoss at the extremes**: `fraction = 0.0` (legal, no-op — bit-identical digests) and `fraction = 1.0` (every packet lost → failsafe, same timing as `RadioLinkLoss`) — `zero_fraction_is_a_no_op`, `full_fraction_equals_radio_link_loss` (Task 4).
4. **BurstLoss window edges**: a packet exactly at the on/off boundary; `on_ms` longer than Betaflight's failsafe timeout triggers a failsafe that recovers when the off-window arrives — `burst_window_boundaries_are_deterministic`, `a_long_burst_failsafes_and_recovers` (Task 4, the recovery half live in Task 7).
5. **Vibration at zero and at standstill**: imbalance set while the motor is stopped (`ω = 0`) contributes exactly zero (no 0/0); the disturbance vanishes the moment imbalance clears — `no_vibration_from_a_stopped_motor`, `imbalance_zero_is_exactly_zero` (Task 2).

## File Structure

```
crates/ofs-core/src/names.rs          + fault/theta/imbalance signal names
crates/ofs-electrical/src/esc_motor.rs + scale seam, theta integration (T1); consumers (T5)
crates/ofs-electrical/src/battery.rs  + additive weak-cell seam (T1); (T6 publishes nothing new)
crates/ofs-physics/src/propeller.rs   + thrust-scale seam (T1); (T5's fault writes from the vehicle)
crates/ofs-sensors/src/imu.rs         + gyro-fault seam (T1); vibration model (T2)
crates/ofs-radio/src/elrs.rs          + fault stream, fraction/burst seams (T1); window math (T4)
crates/ofs-video/src/link.rs          + VtxInterferenceHandle (T1); emitter append (T6)
crates/ofs-sim/src/vehicle.rs         + identity inits, baseline, Fault enum, set_fault/clear_faults,
                                        active_fault_names, battery_cells/has_vtx stash
crates/ofs-sim/src/session.rs         + last_faults edge detector, fault events (T3)
crates/ofs-sim/src/server.rs          + the oneof match arms (T3)
crates/ofs-proto/src/lib.rs           + PROTOCOL_VERSION 8 (T3)
proto/ofs/v1/sim.proto                + Fault oneof, EventKind, State.battery_cell_voltages (T3)
python/ofs/v1/sim_pb2*.py             + regenerated (T3)
python/ofs/client.py                  + PROTOCOL_VERSION 8 (T3)
python/ofs/faults.py                  + the seven dataclasses (T3)
crates/ofs-client/src/model.rs        + FaultInjected/FaultCleared event kinds (T3)
crates/ofs-radio/tests/elrs.rs        + RadioLoss/BurstLoss unit tests (T4)
crates/ofs-electrical/tests/*         + EscMotor/Battery fault tests (T5/T6)
crates/ofs-physics/tests/*            + Propeller thrust-scale test (T5)
crates/ofs-sensors/tests/* (or unit)  + gyro-fault and vibration tests (T2/T6)
crates/ofs-sim/tests/ (or unit)       + validation/clear/digest tests (T3/T5/T6)
python/tests/msp_status.py            new  MSP_STATUS client + arming-disable decode (T7)
python/tests/test_sitl_faults.py      new  live fault tests (T7)
.github/workflows/ci.yml              + the sitl job's pytest list (T7)
docs/research/faults.md               new  constants and estimates (T8)
docs/superpowers/m4b-carried-debt.md  new  (T8)
README.md                             + the fault table grows from one to eight (T8)
```

---

### Task 1: The model seams — signals, theta, identity defaults, zero-behaviour-change

Every model gains the inputs its faults will use; nothing writes a fault value yet. The task's proof is that nothing changed: all existing suites pass unchanged.

**Files:**
- Modify: `crates/ofs-core/src/names.rs`, `crates/ofs-electrical/src/esc_motor.rs`, `crates/ofs-electrical/src/battery.rs`, `crates/ofs-physics/src/propeller.rs`, `crates/ofs-sensors/src/imu.rs`, `crates/ofs-radio/src/elrs.rs`, `crates/ofs-video/src/link.rs`, `crates/ofs-sim/src/vehicle.rs`
- Test: existing suites (must pass unchanged) + new seam-identity assertions in `crates/ofs-sim/src/vehicle.rs`'s `#[cfg(test)]`

**Interfaces:**
- Produces (Tasks 4–6 consume): the signal names of Ruling 2 (+ `prop.{i}.imbalance_kgm`, `motor.{i}.theta_rad`); `VtxInterferenceHandle` with `set(Option<Emitter>)`; the new `Handles` fields — vectors `fault_motor_scale: Vec<Signal<f64>>`, `fault_prop_thrust_scale: Vec<Signal<f64>>`, `prop_imbalance: Vec<Signal<f64>>`, `motor_theta: Vec<Signal<f64>>`, and scalars `fault_radio_loss_fraction`, `fault_burst_on`, `fault_burst_off`, `fault_battery_r_offset`, `fault_battery_v_offset`, `fault_battery_weak_cell`, `fault_imu_gyro_bias: Signal<DVec3>`, `fault_imu_noise_scale` (Tasks 3/5/6 reference exactly these names); `Vehicle` fields `prop_imbalance_baseline: Vec<f64>`, `battery_cells: u32`, `has_vtx: bool`; `ElrsLink` field `fault_rng: ChaCha8Rng`.
- The gyro-fault seam: `Imu::step` reads `fault.imu.gyro_bias_radps` (Vec3) and `fault.imu.noise_scale`; `gyro = rate + p.gyro_bias_radps + fault_bias + noise3(self.gyro_noise) * fault_noise_scale`.

- [ ] **Step 1: Write the seam tests first** (in `vehicle.rs`'s test module — they compile only after Step 2's inits, which is fine: they are the spec for it)

```rust
#[test]
fn fault_signals_start_at_their_identities() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.run_for(0.05).unwrap();
    let b = vehicle.scheduler.bus();
    for i in 0..4 {
        assert_eq!(b.get(vehicle.h.fault_motor_scale[i]), 1.0, "motor {i} scale");
        assert_eq!(b.get(vehicle.h.fault_prop_thrust_scale[i]), 1.0, "prop {i} thrust scale");
        assert_eq!(b.get(vehicle.h.prop_imbalance[i]), 0.0, "prop {i} imbalance (the shipped parts carry none)");
    }
    assert_eq!(b.get(vehicle.h.fault_imu_noise_scale), 1.0);
    assert_eq!(b.get(vehicle.h.fault_radio_loss_fraction), 0.0);
    assert_eq!(b.get(vehicle.h.fault_battery_weak_cell), 0.0);
}

#[test]
fn motors_integrate_theta() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.set_sticks(&Sticks { throttle: 1.0, ..Default::default() });
    vehicle.run_for(0.10).unwrap();
    let b = vehicle.scheduler.bus();
    let theta = b.get(vehicle.h.motor_theta[0]);
    assert!(theta > 0.0 && theta < 2.0 * std::f64::consts::PI, "wrapped into [0, 2π): {theta}");
    let omega = b.get(vehicle.h.omega[0]);
    // 0.10 s at a rising rpm: theta is in the same ballpark as omega·dt, never wildly off.
    assert!(theta < omega * 0.11 + 2.0 * std::f64::consts::PI, "theta consistent with omega: {theta} vs {omega}");
}
```

- [ ] **Step 2: Implement the seams**

`names.rs`:

```rust
/// Fault seams (M4b): the vehicle writes them once per set_fault; models read them each step.
pub const FAULT_RADIO_LOSS_FRACTION: &str = "fault.radio.loss_fraction";
pub const FAULT_RADIO_BURST_ON_S: &str = "fault.radio.burst_on_s";
pub const FAULT_RADIO_BURST_OFF_S: &str = "fault.radio.burst_off_s";

pub fn fault_motor_scale(i: usize) -> String {
    format!("fault.motor.{i}.scale")
}

pub fn fault_prop_thrust_scale(i: usize) -> String {
    format!("fault.prop.{i}.thrust_scale")
}

/// Per-motor static imbalance (part data, overwritten by PropDamage while active).
pub fn prop_imbalance(i: usize) -> String {
    format!("prop.{i}.imbalance_kgm")
}

/// Integrated rotor phase, wrapped to [0, 2π) — the vibration model's phase input.
pub fn motor_theta(i: usize) -> String {
    format!("motor.{i}.theta_rad")
}

pub const FAULT_BATTERY_R_OFFSET_OHM: &str = "fault.battery.r_offset_ohm";
pub const FAULT_BATTERY_V_OFFSET_V: &str = "fault.battery.v_offset_v";
/// Cell index + 1; 0 = no weak cell.
pub const FAULT_BATTERY_WEAK_CELL: &str = "fault.battery.weak_cell";

pub const FAULT_IMU_GYRO_BIAS: &str = "fault.imu.gyro_bias_radps";
pub const FAULT_IMU_NOISE_SCALE: &str = "fault.imu.noise_scale";
```

`esc_motor.rs`: `EscMotor` gains `fault_scale: Signal<f64>` and `theta_sig: Signal<f64>` (registered in `new` via `names::fault_motor_scale(index)` / `names::motor_theta(index)`), a `theta: f64` field, and in `step` after the clamp line:

```rust
let scale = bus.get(self.fault_scale);
let current = (((applied_v - self.ke * self.omega) / self.motor.resistance_ohm).clamp(-limit, limit)) * scale;
```

(the doc comment's current formula gains "× fault scale"), publishes `current` (the scaled value) as today, and before the publishes:

```rust
self.theta = (self.theta + self.omega * dt) % (2.0 * std::f64::consts::PI);
```

publishing it on `theta_sig`.

`battery.rs`: `Battery` gains `r_offset: Signal<f64>`, `v_offset: Signal<f64>`, `weak_cell: Signal<f64>` (names above); `publish` becomes:

```rust
fn publish(&self, bus: &mut Bus, current: f64) {
    let ocv = f64::from(self.p.cells) * interp::linear(&self.p.ocv_table, self.soc);
    let droop = current * (self.p.r0_ohm + bus.get(self.r_offset)) + bus.get(self.v_offset);
    bus.set(self.voltage, ocv - current * self.p.r0_ohm - self.v1 - droop);
    ...unchanged...
}
```

(Careful — write it exactly: the old `current * r0` term must not be applied twice. The full expression is `ocv − I·(r0 + Δr) − v1 − Δv`.)

`propeller.rs`: `Propeller` gains `thrust_scale: Signal<f64>`; `step`:

```rust
let (t, q) = thrust_torque(&self.p, bus.get(self.omega));
let scale = bus.get(self.thrust_scale);
bus.set(self.thrust, t * scale);
bus.set(self.torque, q * scale);
```

`imu.rs`: `Imu` gains `fault_bias: Signal<DVec3>` and `fault_noise_scale: Signal<f64>`; `step`'s gyro line per the Interfaces block above (accel unchanged — the gyro fault is gyro-only).

`elrs.rs`: `ElrsLink` gains `fault_rng: ChaCha8Rng` (from `model_rng(seed, "elrs-faults")` — the seed comes in via `new`'s existing `seed` param) and signals `fault_loss_fraction`, `fault_burst_on`, `fault_burst_off`. In `step`, immediately after the existing uplink loss draw (`let draw: f64 = self.rng.gen();`), in this exact position:

```rust
let fault_draw: f64 = self.fault_rng.gen();
let fraction = bus.get(self.fault_loss_fraction);
let burst_on = bus.get(self.fault_burst_on);
let burst_off = bus.get(self.fault_burst_off);
let in_burst = burst_on > 0.0 && ctx.time_s % (burst_on + burst_off) < burst_on;
let received = transmitting && draw >= packet_error_rate(rssi[active], sensitivity)
    && fault_draw >= fraction && !in_burst;
```

(The `received` line replaces the existing one; the downlink path is untouched. `in_burst` lives here already in Task 1 as inert seam code — burst signals are bus-zero, so `in_burst` is always false; Task 4 only adds its tests. Note the spec's RadioLoss row says "extra per-packet Bernoulli loss… like RadioLinkLoss"; `BurstLoss` loses every uplink packet in its window, so both fold into the same `received` conjunction.)

`link.rs`: the handle and its consumption:

```rust
/// A runtime emitter injected by the VtxInterference fault (the OsdHandle pattern: shared state the
/// vehicle writes and the model reads). Default = no injected emitter.
#[derive(Clone, Default)]
pub struct VtxInterferenceHandle(std::sync::Arc<std::sync::Mutex<Option<Emitter>>>);

impl VtxInterferenceHandle {
    pub fn set(&self, emitter: Option<Emitter>) {
        *self.0.lock().expect("vtx interference handle poisoned") = emitter;
    }
    fn emitter(&self) -> Option<Emitter> {
        *self.0.lock().expect("vtx interference handle poisoned")
    }
}
```

`VideoLink::new(params, divisor, seed, handle, bus)` gains the parameter (stored on the struct); `interference_for` computes over an extended emitter list:

```rust
let mut emitters = self.params.world.emitters.clone();
if let Some(e) = self.handle.emitter() {
    emitters.push(e);
}
let values = interference_dbm(&LinkWorld { emitters, ..self.params.world.clone() }, freq_mhz, self.params.ground_bounce);
```

(the cached `self.interference` keyed on frequency is unchanged — the handle changes only at `set_fault` time, outside the tick, so per-frequency caching stays correct). **All three existing `VideoLink::new` call sites gain the argument** — `crates/ofs-video/tests/link.rs:156`, `:315` (pass `VtxInterferenceHandle::default()`), and `crates/ofs-sim/src/vehicle.rs:706` (pass the vehicle-owned handle).

`vehicle.rs`: `Handles` gains the new signal vectors/scalars registered like `omega`; `build` stores `prop_imbalance_baseline: cfg.props.iter().map(|p| p.imbalance_kgm).collect()`, `battery_cells: cfg.battery.cells`, `has_vtx: cfg.vtx.is_some()`, and after the closing `vehicle.set_transmitter(true)`:

```rust
vehicle.init_fault_identities(&cfg.props.iter().map(|p| p.imbalance_kgm).collect::<Vec<_>>());
```

as a private method beside `set_sticks` (the same borrow shape):

```rust
/// Fault seams start at their identities: multiplicative faults at 1.0, the prop imbalance at its
/// part data, additive faults at the bus's zero.
fn init_fault_identities(&mut self, imbalance: &[f64]) {
    let h = &self.h;
    let bus = self.scheduler.bus_mut();
    for (i, imbalance) in imbalance.iter().enumerate() {
        bus.set(h.fault_motor_scale[i], 1.0);
        bus.set(h.fault_prop_thrust_scale[i], 1.0);
        bus.set(h.prop_imbalance[i], *imbalance);
    }
    bus.set(h.fault_imu_noise_scale, 1.0);
}
```

(`clear_faults` in Task 3 reuses this method with the stored baseline — the single place that knows the identity values.) The `ElrsLink::new` call site passes the new handle through from a handle constructed inside `build`.

- [ ] **Step 3: Run everything**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: all PASS unchanged (the new Step 1 tests included), clippy clean. Any pinned expectation that moved is a defect in the seam — fix the seam, not the expectation.

- [ ] **Step 4: Commit**

```bash
git add crates/
git commit -m "feat(sim): the fault seams - fault signals at their identities, theta integration, the runtime-emitter handle"
```

---

### Task 2: The IMU vibration model

Deterministic, phase-driven disturbance from per-motor imbalance (spec §3 "motor-induced vibration from imbalance"; Ruling 8's formulas).

**Files:**
- Modify: `crates/ofs-sensors/src/imu.rs`
- Test: `crates/ofs-sensors/tests/vibration.rs` (new)

**Interfaces:**
- Consumes: `prop.{i}.imbalance_kgm`, `motor.{i}.theta_rad`, `motor.{i}.omega_radps` (all T1), the frame's motor positions and mass/inertia (passed at construction).
- Produces: `ImuParams` gains `vibration: Option<VibrationParams>` where `pub struct VibrationParams { pub positions_frd: Vec<DVec3>, pub mass_kg: f64, pub inertia_kgm2: DVec3 }`; the IMU reads per-motor imbalance/theta/omega signals by name (registered in `new` like its other signals).

- [ ] **Step 1: Write the failing tests** (new file `crates/ofs-sensors/tests/vibration.rs`; the IMU registers its inputs by name on an empty bus, and the outputs are read back through `names::IMU_GYRO`/`names::IMU_ACCEL`)

```rust
use glam::DVec3;
use ofs_core::{names, Bus};
use ofs_sensors::imu::{Imu, ImuParams, VibrationParams};

const DT: f64 = 1.0 / 8000.0;
const OMEGA: f64 = 300.0; // ~2862 rpm

fn vib() -> VibrationParams {
    VibrationParams {
        positions_frd: vec![DVec3::new(0.08, 0.08, 0.0)],
        mass_kg: 0.65,
        inertia_kgm2: DVec3::new(0.0025, 0.0025, 0.0045),
    }
}

fn params(vibration: Option<VibrationParams>) -> ImuParams {
    ImuParams {
        gyro_noise_std_radps: 0.0, // deterministic runs: the disturbance must stand alone
        gyro_bias_radps: DVec3::ZERO,
        accel_noise_std_mps2: 0.0,
        accel_bias_mps2: DVec3::ZERO,
        vibration,
    }
}

/// Runs 400 IMU steps (0.05 s) with one motor at `omega` and `imbalance`; returns the last (gyro, accel).
fn run(vibration: Option<VibrationParams>, omega: f64, imbalance: f64) -> (DVec3, DVec3) {
    let mut bus = Bus::new();
    bus.set(bus.signal::<f64>("motor.0.omega_radps"), omega);
    bus.set(bus.signal::<f64>("prop.0.imbalance_kgm"), imbalance);
    let mut imu = Imu::new(params(vibration), 7, &mut bus);
    for _ in 0..400 {
        imu.step(&ofs_core::StepCtx { tick: 0, time_s: 0.0, dt_s: DT }, &mut bus).unwrap();
    }
    let gyro = bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap());
    let accel = bus.get(bus.lookup::<DVec3>(names::IMU_ACCEL).unwrap());
    (gyro, accel)
}

#[test]
fn imbalance_zero_is_exactly_zero() {
    let (gyro, accel) = run(Some(vib()), OMEGA, 0.0);
    assert_eq!(gyro, DVec3::ZERO, "no noise, no bias, no imbalance");
    // A level body: specific force is +g on the body z (the IMU feels the reaction to gravity).
    assert!((accel.z - ofs_core::consts::GRAVITY_MPS2).abs() < 1e-9, "{accel:?}");
    assert_eq!(accel.x, 0.0, "{accel:?}");
    assert_eq!(accel.y, 0.0, "{accel:?}");
}

#[test]
fn a_stopped_motor_contributes_exactly_zero() {
    let (gyro, _) = run(Some(vib()), 0.0, 1.0e-3);
    assert_eq!(gyro, DVec3::ZERO, "omega 0: frozen phase, zero force, no 0/0");
}

#[test]
fn the_rotating_force_has_the_analytic_magnitude() {
    let (_, accel) = run(Some(vib()), OMEGA, 1.0e-3);
    let f = 1.0e-3 * OMEGA * OMEGA; // (m·r)·ω² in N
    let specific = f / 0.65; // m/s², the body-frame xy magnitude
    let magnitude = accel.x.hypot(accel.y);
    assert!((magnitude - specific).abs() < specific * 0.05, "xy specific force {magnitude} vs analytic {specific}");
}

#[test]
fn the_rate_disturbance_is_bounded_by_the_analytic_amplitude() {
    let (gyro, _) = run(Some(vib()), OMEGA, 1.0e-3);
    let f = 1.0e-3 * OMEGA * OMEGA;
    let moment = 0.08_f64.hypot(0.08) * f; // r ⊥ F in the worst case
    let rate_amp = moment / (0.0025 * OMEGA);
    let gyro_xy = gyro.x.hypot(gyro.y);
    assert!(gyro_xy <= rate_amp * 1.05, "rate disturbance {gyro_xy} vs analytic amplitude {rate_amp}");
    assert!(gyro_xy > 0.0, "the disturbance is present");
}

#[test]
fn a_motor_without_vibration_params_stays_clean() {
    let (gyro, accel) = run(None, OMEGA, 1.0e-3);
    assert_eq!(gyro, DVec3::ZERO);
    assert_eq!(accel.x, 0.0);
    assert_eq!(accel.y, 0.0);
}
```

(The xy-magnitude assertion uses the rotating force's invariant: the instantaneous x/y components vary with phase but their hypot does not. The rate assertion is an upper bound because the sampled phase may align the moment partly with z. If the implemented sign convention differs, the magnitude tests still hold — document the chosen sign in a comment as "the imbalance force pushes the airframe; the IMU rides the airframe".)

- [ ] **Step 2: Run to verify they fail** — `cargo test -p ofs-sensors` — Expected: FAIL (`VibrationParams` not defined).

- [ ] **Step 3: Implement** — `VibrationParams` (per the Interfaces block), signal registration (imbalance/theta/omega per motor up to `positions_frd.len()`), and the disturbance in `step` before publishing:

```rust
if let Some(v) = &self.p.vibration {
    let mass_recip = 1.0 / v.mass_kg;
    for (i, pos) in v.positions_frd.iter().enumerate() {
        let imbalance = bus.get(self.imbalance[i]);
        if imbalance <= 0.0 {
            continue;
        }
        let omega = bus.get(self.omega[i]);
        if omega <= 0.0 {
            continue; // a stopped motor contributes exactly zero (no 0/0 in the rate term)
        }
        let theta = bus.get(self.theta[i]);
        let alpha = pos.y.atan2(pos.x);
        let phase = theta + alpha;
        let f = imbalance * omega * omega;
        let (sin, cos) = phase.sin_cos();
        force_frd += DVec3::new(f * cos, f * sin, 0.0);
        // Rotating moment about the CoM from the lever arm; its angular-rate amplitude.
        let moment_xy = DVec3::new(pos.x, pos.y, 0.0).cross(DVec3::new(f * cos, f * sin, 0.0));
        rate_disturb += DVec3::new(
            moment_xy.x / v.inertia_kgm2.x / omega,
            moment_xy.y / v.inertia_kgm2.y / omega,
            0.0, // the prop plane's force has no z lever in v1
        );
    }
    gyro += rate_disturb;
}
```

and the force reaches the IMU's specific force in the body frame: `accel += att.inverse() * (-force_frd * mass_recip)`? No — `force_frd` is already body-frame (FRD), and the IMU publishes body-frame specific force; the vibrating mass pushes the airframe, the IMU sees the reaction: `accel += force_frd * mass_recip` with the sign such that the xy magnitude matches (pin the sign so `the_rotating_force_has_the_analytic_magnitude` passes — the magnitude assertion is sign-agnostic; document the chosen sign in a comment as "the imbalance force pushes the airframe; the IMU rides the airframe").

`vehicle.rs`'s `build` constructs `VibrationParams` from `cfg.frame.motor_positions_frd_m`, `cfg.frame.mass_kg`, `cfg.frame.inertia_kgm2` whenever there are motors — always `Some` for v1 quads (4 motors validated); a `None` path exists for the empty-motor edge the M4a fix wave pinned.

- [ ] **Step 4: Run** — `cargo test -p ofs-sensors && cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS, and the no-imbalance runs are bit-identical to Task 1's (the `imbalance <= 0.0` early-continue guards that).

- [ ] **Step 5: Commit** — `git commit -m "feat(sensors): the vibration model - per-motor imbalance as a rotating force and rate disturbance"`

---

### Task 3: Fault plumbing — protocol 8, the Fault enum, events, Python

All wire changes in one task. After it: every fault *validates and activates* (writing its signals), the events fire, Python can inject — but only `RadioLinkLoss` has behaviour beyond signal writes; the rest become live with Tasks 4–6.

**Files:**
- Modify: `proto/ofs/v1/sim.proto`, `crates/ofs-proto/src/lib.rs`, `crates/ofs-sim/src/vehicle.rs`, `crates/ofs-sim/src/session.rs`, `crates/ofs-sim/src/server.rs`, `crates/ofs-client/src/model.rs`, `python/ofs/client.py`, `python/ofs/faults.py`, `python/ofs/v1/sim_pb2*` (regenerated)
- Test: `crates/ofs-sim/src/vehicle.rs` tests, `crates/ofs-sim/src/session.rs` tests, `python/tests/test_client.py`

**Interfaces:**
- Consumes: Task 1's signals and `VtxInterferenceHandle::set`; `Vehicle`'s T1 fields.
- Produces: `vehicle::Fault` enum (below); `Vehicle::{set_fault(&mut self, fault: Fault, active: bool), clear_faults(&mut self), active_fault_names(&self) -> Vec<String>}`; proto `Fault` oneof with `radio_link_loss = 1`, `radio_loss = 2`, `burst_loss = 3`, `motor_failure = 4`, `prop_damage = 5`, `weak_cell = 6`, `gyro_fault = 7`, `vtx_interference = 8`; `EventKind` values 14 (`EVENT_KIND_FAULT_INJECTED`), 15 (`EVENT_KIND_FAULT_CLEARED`); `State.battery_cell_voltages` (`repeated double`, field number 20 — verify 20 is free in `sim.proto` and take the next free if not); python `ofs.faults.{RadioLoss, BurstLoss, MotorFailure, PropDamage, WeakCell, GyroFault, VtxInterference}` dataclasses with `_to_pb`.

- [ ] **Step 1: The proto changes** (`proto/ofs/v1/sim.proto`):

```protobuf
message RadioLoss {
  double fraction = 1;  // extra per-packet loss probability, [0, 1]
}

message BurstLoss {
  uint32 on_ms = 1;   // the window loses every uplink packet
  uint32 off_ms = 2;  // then delivers normally; both > 0
}

message MotorFailure {
  uint32 motor = 1;   // 0-based motor index
  double scale = 2;   // torque/current scale, [0, 1]; 0 = dead motor
}

message PropDamage {
  uint32 prop = 1;            // 0-based prop index
  double thrust_scale = 2;    // thrust and torque scale, >= 0
  double imbalance_kgm = 3;   // vibration amplitude (m·r), >= 0
}

message WeakCell {
  uint32 cell = 1;            // 0-based cell index
  double r_offset_ohm = 2;    // extra series resistance
  double v_offset_v = 3;      // extra voltage drop
}

message GyroFault {
  Vec3 bias_radps = 1;       // added to the IMU's gyro bias
  double noise_scale = 2;    // multiplies the gyro noise std, >= 0
}

message VtxInterference {
  double freq_mhz = 1;
  double power_mw = 2;
  Vec3 position_ned_m = 3;
}

message Fault {
  oneof kind {
    RadioLinkLoss radio_link_loss = 1;
    RadioLoss radio_loss = 2;
    BurstLoss burst_loss = 3;
    MotorFailure motor_failure = 4;
    PropDamage prop_damage = 5;
    WeakCell weak_cell = 6;
    GyroFault gyro_fault = 7;
    VtxInterference vtx_interference = 8;
  }
}
```

`EventKind` gains `EVENT_KIND_FAULT_INJECTED = 14;` and `EVENT_KIND_FAULT_CLEARED = 15;` with the file's comment style. `State` gains `repeated double battery_cell_voltages = 19;` (field 18, `fc_telemetry`, is the last one today). Regenerate the stubs with the Global Constraints recipe. `python/ofs/client.py`: `PROTOCOL_VERSION = 8`; `State` dataclass gains `battery_cell_voltages: tuple = ()`, `_state` maps it.

- [ ] **Step 2: The vehicle-side enum, writes, and validation** (`crates/ofs-sim/src/vehicle.rs`):

```rust
/// Faults a script can inject (spec §3). Names here are the `active_fault_names` strings.
#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    RadioLinkLoss,
    RadioLoss { fraction: f64 },
    BurstLoss { on_s: f64, off_s: f64 },
    MotorFailure { motor: usize, scale: f64 },
    PropDamage { prop: usize, thrust_scale: f64, imbalance_kgm: f64 },
    WeakCell { cell: usize, r_offset_ohm: f64, v_offset_v: f64 },
    GyroFault { bias_radps: DVec3, noise_scale: f64 },
    VtxInterference { freq_mhz: f64, power_mw: f64, position_ned_m: DVec3 },
}

impl Fault {
    fn name(&self) -> &'static str {
        match self {
            Fault::RadioLinkLoss(_) | Fault::RadioLoss { .. } => "radio_loss", // RadioLinkLoss is "radio_link_loss" — see below
            ...
        }
    }
}
```

(Write the real `name()`: `RadioLinkLoss => "radio_link_loss"`, `RadioLoss => "radio_loss"`, `BurstLoss => "burst_loss"`, `MotorFailure => "motor_failure"`, `PropDamage => "prop_damage"`, `WeakCell => "weak_cell"`, `GyroFault => "gyro_fault"`, `VtxInterference => "vtx_interference"` — the plan's snippet above is shape only, not the mapping.)

`set_fault` validates then writes:

```rust
pub fn set_fault(&mut self, fault: Fault, active: bool) -> Result<(), SimError> {
    let b = self.scheduler.bus_mut();
    match &fault {
        Fault::RadioLinkLoss => b.set(self.h.fault_radio_loss, if active { 1.0 } else { 0.0 }),
        Fault::RadioLoss { fraction } => {
            if !(0.0..=1.0).contains(fraction) {
                return Err(SimError::InvalidArgument(format!("radio loss fraction must be in [0, 1], got {fraction}")));
            }
            b.set(self.h.fault_radio_loss_fraction, if active { *fraction } else { 0.0 });
        }
        Fault::BurstLoss { on_s, off_s } => {
            if *on_s <= 0.0 || *off_s <= 0.0 {
                return Err(SimError::InvalidArgument(format!("burst on_ms and off_ms must both be > 0, got on {on_s} s / off {off_s} s")));
            }
            let (on, off) = if active { (*on_s, *off_s) } else { (0.0, 0.0) };
            b.set(self.h.fault_burst_on, on);
            b.set(self.h.fault_burst_off, off);
        }
        Fault::MotorFailure { motor, scale } => {
            if *motor >= self.h.fault_motor_scale.len() {
                return Err(SimError::InvalidArgument(format!("motor {motor} does not exist (the quad has {})", self.h.fault_motor_scale.len())));
            }
            if !(0.0..=1.0).contains(scale) {
                return Err(SimError::InvalidArgument(format!("motor failure scale must be in [0, 1], got {scale}")));
            }
            b.set(self.h.fault_motor_scale[*motor], if active { *scale } else { 1.0 });
        }
        Fault::PropDamage { prop, thrust_scale, imbalance_kgm } => {
            if *prop >= self.h.fault_prop_thrust_scale.len() {
                return Err(SimError::InvalidArgument(format!("prop {prop} does not exist (the quad has {})", self.h.fault_prop_thrust_scale.len())));
            }
            if *thrust_scale < 0.0 || *imbalance_kgm < 0.0 {
                return Err(SimError::InvalidArgument(format!("prop damage thrust_scale and imbalance_kgm must be >= 0, got {thrust_scale} / {imbalance_kgm}")));
            }
            b.set(self.h.fault_prop_thrust_scale[*prop], if active { *thrust_scale } else { 1.0 });
            b.set(self.h.prop_imbalance[*prop], if active { *imbalance_kgm } else { self.prop_imbalance_baseline[*prop] });
        }
        Fault::WeakCell { cell, r_offset_ohm, v_offset_v } => {
            if *cell as u32 >= self.battery_cells {
                return Err(SimError::InvalidArgument(format!("cell {cell} does not exist (the pack has {} cells)", self.battery_cells)));
            }
            if *r_offset_ohm < 0.0 || *v_offset_v < 0.0 {
                return Err(SimError::InvalidArgument("weak-cell offsets must be >= 0".into()));
            }
            b.set(self.h.fault_battery_weak_cell, if active { *cell as f64 + 1.0 } else { 0.0 });
            b.set(self.h.fault_battery_r_offset, if active { *r_offset_ohm } else { 0.0 });
            b.set(self.h.fault_battery_v_offset, if active { *v_offset_v } else { 0.0 });
        }
        Fault::GyroFault { bias_radps, noise_scale } => {
            if *noise_scale < 0.0 || !bias_radps.is_finite() {
                return Err(SimError::InvalidArgument("gyro fault noise_scale must be >= 0 and bias finite".into()));
            }
            b.set(self.h.fault_imu_gyro_bias, if active { *bias_radps } else { DVec3::ZERO });
            b.set(self.h.fault_imu_noise_scale, if active { *noise_scale } else { 1.0 });
        }
        Fault::VtxInterference { freq_mhz, power_mw, position_ned_m } => {
            if !self.has_vtx {
                return Err(SimError::InvalidArgument("vtx interference needs a quad with a [vtx] section".into()));
            }
            if *freq_mhz <= 0.0 || *power_mw <= 0.0 || !position_ned_m.is_finite() {
                return Err(SimError::InvalidArgument(format!("vtx interference needs a positive frequency and power and a finite position, got {freq_mhz} MHz / {power_mw} mW / {position_ned_m:?}")));
            }
            self.vtx_interference.set(active.then(|| Emitter {
                position: *position_ned_m,
                freq_mhz: *freq_mhz,
                power_mw: *power_mw,
                antenna: Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Rhcp, axis: DVec3::NEG_Z },
            }));
        }
    }
    self.record_fault_active(&fault, active); // maintains self.active_faults: Vec<String>
    Ok(())
}
```

(`Vehicle` gains `active_faults: Vec<String>`; `record_fault_active` inserts `fault.name()` when `active` (replacing an existing same-name entry, so a re-inject updates parameters without duplicating) and removes it when not. `active_fault_names()` returns the list sorted in the spec table's order — `radio_link_loss`, `radio_loss`, `burst_loss`, `motor_failure`, `prop_damage`, `weak_cell`, `gyro_fault`, `vtx_interference` — so the event stream is deterministic regardless of injection order. Unit-test `record_fault_active` + the sort with the three-name sequence in Step 5.)

`state()` gains the synthesis (Ruling 7):

```rust
let weak = b.get(h.fault_battery_weak_cell);
let cells = self.battery_cells.max(1) as f64;
let share = s.battery_voltage_v / cells;
let droop_share = b.get(h.fault_battery_r_offset) * s.battery_current_a
    + b.get(h.fault_battery_v_offset);
s.battery_cell_voltages = (0..self.battery_cells)
    .map(|c| if Some(c) == weak_cell_index(weak) { share - droop_share } else { share })
    .collect();
```

(`weak_cell_index(weak) = (weak > 0.0).then(|| weak as usize - 1)`.) `VehicleState` gains `battery_cell_voltages: Vec<f64>`.

- [ ] **Step 3: Session events and the server match** (`session.rs`): `Session` gains `last_faults: Vec<String>`; `changes()` fetches `self.vehicle.active_fault_names()` before `events_for` and passes it in; `events_for` compares and raises:

```rust
for name in &faults {
    if !self.last_faults.contains(name) {
        out.push(event(s.time_s, pb::EventKind::FaultInjected, name.clone()));
    }
}
for name in &self.last_faults {
    if !faults.contains(name) {
        out.push(event(s.time_s, pb::EventKind::FaultCleared, name.clone()));
    }
}
self.last_faults = faults;
```

`server.rs`'s `inject_fault` (today it constructs `pb::Fault` → the single `RadioLinkLoss` variant; find the match) gains the seven arms mapping oneof members to `vehicle::Fault` fields verbatim (Vec3 → `DVec3::new(m.x, m.y, m.z)` via the existing `_v`-style helper if one exists in that file) and propagates `set_fault`'s error as `Status::invalid_argument`. `clear_faults` unchanged in shape (calls the new `clear_faults`). `ofs-client/src/model.rs`: `EventKind::{FaultInjected, FaultCleared}` with `as_str` "fault_injected"/"fault_cleared" and the two `from_pb` arms.

- [ ] **Step 4: Python dataclasses** (`python/ofs/faults.py`):

```python
@dataclass(frozen=True)
class RadioLoss:
    """Extra per-packet uplink loss while active: `fraction` of packets are dropped on top of the link model's own."""

    fraction: float

    def _to_pb(self):
        return pb.Fault(radio_loss=pb.RadioLoss(fraction=self.fraction))


@dataclass(frozen=True)
class BurstLoss:
    """Every uplink packet is lost for `on_ms`, then the link delivers normally for `off_ms`, repeating from t=0."""

    on_ms: int
    off_ms: int

    def _to_pb(self):
        return pb.Fault(burst_loss=pb.BurstLoss(on_ms=self.on_ms, off_ms=self.off_ms))


@dataclass(frozen=True)
class MotorFailure:
    """Motor `motor` (0-based) produces `scale` of its torque and current; 0 = dead motor."""

    motor: int
    scale: float = 0.0

    def _to_pb(self):
        return pb.Fault(motor_failure=pb.MotorFailure(motor=self.motor, scale=self.scale))


@dataclass(frozen=True)
class PropDamage:
    """Propeller `prop` (0-based) produces `thrust_scale` of its thrust and torque, with `imbalance_kgm` of vibration."""

    prop: int
    thrust_scale: float = 1.0
    imbalance_kgm: float = 0.0

    def _to_pb(self):
        return pb.Fault(prop_damage=pb.PropDamage(prop=self.prop, thrust_scale=self.thrust_scale,
                                                  imbalance_kgm=self.imbalance_kgm))


@dataclass(frozen=True)
class WeakCell:
    """Cell `cell` (0-based) carries `r_offset_ohm` of extra series resistance and `v_offset_v` of extra droop."""

    cell: int
    r_offset_ohm: float = 0.01
    v_offset_v: float = 0.05

    def _to_pb(self):
        return pb.Fault(weak_cell=pb.WeakCell(cell=self.cell, r_offset_ohm=self.r_offset_ohm,
                                              v_offset_v=self.v_offset_v))


@dataclass(frozen=True)
class GyroFault:
    """The IMU's gyro reads `bias_radps` of extra bias and `noise_scale` times its normal noise."""

    bias_radps: tuple = (0.0, 0.0, 0.0)
    noise_scale: float = 1.0

    def _to_pb(self):
        x, y, z = self.bias_radps
        return pb.Fault(gyro_fault=pb.GyroFault(bias_radps=pb.Vec3(x=x, y=y, z=z), noise_scale=self.noise_scale))


@dataclass(frozen=True)
class VtxInterference:
    """A transmitter at `position_ned_m` on `freq_mhz` at `power_mw` interferes with the video link while active."""

    freq_mhz: float
    power_mw: float
    position_ned_m: tuple

    def _to_pb(self):
        x, y, z = self.position_ned_m
        return pb.Fault(vtx_interference=pb.VtxInterference(freq_mhz=self.freq_mhz, power_mw=self.power_mw,
                                                            position_ned_m=pb.Vec3(x=x, y=y, z=z)))
```

(Update the module docstring's "the v1 catalog completes in M4" line to the done state. `RadioLinkLoss` keeps its existing shape but drops its now-stale trailing `_` — keep the existing dataclass exactly as it is otherwise.)

- [ ] **Step 5: Tests**

`vehicle.rs` (or `ofs-sim` integration tests, following where the existing vehicle tests live):

```rust
#[test]
fn an_unknown_motor_index_is_invalid_argument() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    let err = vehicle.set_fault(Fault::MotorFailure { motor: 7, scale: 0.0 }, true).unwrap_err();
    assert!(err.to_string().contains("motor 7"), "{err}");
    assert!(err.to_string().contains("does not exist"), "{err}");
    assert_eq!(vehicle.active_fault_names(), Vec::<String>::new(), "a rejected fault is not recorded");
}

#[test]
fn vtx_interference_without_a_vtx_is_invalid_argument() {
    let mut cfg = shipped_quad();
    cfg.vtx = None;
    let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
    let err = vehicle.set_fault(Fault::VtxInterference { freq_mhz: 5700.0, power_mw: 100.0, position_ned_m: DVec3::X }, true).unwrap_err();
    assert!(err.to_string().contains("[vtx]"), "{err}");
}

#[test]
fn injected_and_cleared_faults_are_named_in_order() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.set_fault(Fault::WeakCell { cell: 2, r_offset_ohm: 0.01, v_offset_v: 0.05 }, true).unwrap();
    vehicle.set_fault(Fault::RadioLinkLoss, true).unwrap();
    assert_eq!(vehicle.active_fault_names(), vec!["radio_link_loss".to_string(), "weak_cell".to_string()]);
    vehicle.clear_faults();
    assert!(vehicle.active_fault_names().is_empty());
}
```

`session.rs` tests (following its existing hand-built-state pattern): an injected fault raises `fault_injected` once, a cleared one `fault_cleared` once, and no events fire while the set is unchanged. `python/tests/test_client.py`: a `RadioLoss(0.5)._to_pb()` round-trips through the regenerated pb (`pb.Fault.WhichOneof("kind") == "radio_loss"`).

- [ ] **Step 6: Run everything** — `cargo test --workspace`, clippy, regenerate check (stub files committed), and the python unit suite (`uv run --with grpcio,protobuf,pytest pytest python/tests/test_client.py python/tests/test_realtime.py`).

- [ ] **Step 7: Commit**

```bash
git add proto/ crates/ python/ofs/
git commit -m "feat(proto)!: protocol 8 - the v1 fault catalog's wire shape, events and python dataclasses"
```

---

### Task 4: RadioLoss and BurstLoss — the radio faults behave

The ELRS seam from Task 1 already folds both into `received`; this task proves the behaviour and pins the window math.

**Files:**
- Test: `crates/ofs-radio/tests/elrs.rs` (extend; read its existing harness first — it builds an `ElrsLink` against a hand-fed bus)

**Interfaces:**
- Consumes: Task 1's seam (signals + `fault_rng`); Task 3's `set_fault`.
- Produces: nothing new — behaviour only.

- [ ] **Step 1: Write the failing tests** (extend `crates/ofs-radio/tests/elrs.rs`, using its `rig`/`set`/`get`/`packets`/`statistics` harness):

```rust
#[test]
fn zero_fraction_is_a_no_op() {
    let (mut clean, ..) = rig(LinkParams::ideal(500), 11);
    let (mut faulted, ..) = rig(LinkParams::ideal(500), 11);
    set(&mut faulted, names::FAULT_RADIO_LOSS_FRACTION, 0.0);
    packets(&mut clean.0, 500);
    packets(&mut faulted.0, 500);
    // The dedicated fault stream consumed draws; the model's own stream did not move, so the
    // LQ histories agree exactly.
    close(get(&clean.0, names::RADIO_LQ), get(&faulted.0, names::RADIO_LQ), 0.0, "LQ identical");
}

#[test]
fn full_fraction_equals_radio_link_loss() {
    let (mut cut, ..) = rig(LinkParams::ideal(500), 3);
    set(&mut cut, names::FAULT_RADIO_LINK_LOSS, 1.0);
    packets(&mut cut, 300);
    let (mut full, ..) = rig(LinkParams::ideal(500), 3);
    set(&mut full, names::FAULT_RADIO_LOSS_FRACTION, 1.0);
    packets(&mut full, 300);
    assert_eq!(get(&cut, names::RADIO_LQ), 0.0);
    assert_eq!(get(&full, names::RADIO_LQ), 0.0, "every packet lost: the same silence");
    assert_eq!(get(&full, names::RADIO_LINK_UP), 0.0);
}

#[test]
fn half_fraction_drops_about_half() {
    let mut s = rig(LinkParams::ideal(500), 5).0;
    set(&mut s, names::FAULT_RADIO_LOSS_FRACTION, 0.5);
    packets(&mut s, 2000);
    let lq = get(&s, names::RADIO_LQ);
    assert!(lq > 35.0 && lq < 65.0, "binomial 3-sigma band around 50: {lq}");
}

#[test]
fn burst_window_boundaries_are_deterministic() {
    // 100 ms on / 300 ms off at 500 Hz: 50 lost packets per on-phase, 150 delivered per off-phase.
    // LQ is the last 100 packets, so the off-phase's tail reads a full clean window.
    let mut s = rig(LinkParams::ideal(500), 9).0;
    set(&mut s, names::FAULT_RADIO_BURST_ON_S, 0.1);
    set(&mut s, names::FAULT_RADIO_BURST_OFF_S, 0.3);
    packets(&mut s, 50);
    assert_eq!(get(&s, names::RADIO_LQ), 0.0, "the on-window lost every packet so far");
    packets(&mut s, 150);
    assert!(get(&s, names::RADIO_LQ) > 90.0, "the 150-packet off-window flushed the history");
    packets(&mut s, 50);
    close(get(&s, names::RADIO_LQ), 50.0, 1.0, "the second on-window mixes 50 lost into the last 100");
}

#[test]
fn a_long_burst_failsafes_and_recovers() {
    let mut s = rig(LinkParams::ideal(500), 13).0;
    set(&mut s, names::FAULT_RADIO_BURST_ON_S, 2.0);
    set(&mut s, names::FAULT_RADIO_BURST_OFF_S, 1.0);
    packets(&mut s, 900); // 1.8 s: inside the 2 s on-window
    assert_eq!(get(&s, names::RADIO_LINK_UP), 0.0, "LQ hit zero during the burst");
    packets(&mut s, 500); // 1.0 s more: 2.8 s, inside the off-window
    assert_eq!(get(&s, names::RADIO_LINK_UP), 1.0, "the off-window delivered again");
}
```

(The `names::FAULT_*` constants are Task 1's; `LinkParams::ideal(500)` is the file's existing ideal-link constructor. If the LQ window's 100-packet smoothing makes a boundary assertion off by a packet or two, widen the band — the determinism pin is that the same seed replays the same windows, which `zero_fraction_is_a_no_op` also guards.)

- [ ] **Step 2: Verify fail, then implement** — the seam is inert until `fraction`/burst signals are written; the tests write them directly on the harness bus (the harness already writes `tx_enabled`-style inputs). No `elrs.rs` logic changes are expected beyond what Task 1 landed — if a test reveals the seam is wrong (e.g. the burst check misses the packet edge), fix `elrs.rs` here and say so in the report.

- [ ] **Step 3: Run, clippy, commit**

```bash
git add crates/ofs-radio/
git commit -m "feat(radio): the radio loss faults - fractional loss and the deterministic burst window"
```

---

### Task 5: MotorFailure and PropDamage — the powertrain faults behave

**Files:**
- Test: `crates/ofs-electrical/tests/` (extend or add `faults.rs`), `crates/ofs-physics/tests/` (extend), `crates/ofs-sim/src/vehicle.rs` tests (the clear-restores-baseline + digest-rejoin pins)

**Interfaces:**
- Consumes: Task 1's scale/theta seams; Task 3's `set_fault`/`clear_faults` and the baseline restore.
- Produces: nothing new — behaviour only.

- [ ] **Step 1: Write the failing tests** (the electrical/physics harnesses: `sim(volts, cmd, no_load_a)` + `get` in `esc_motor.rs`'s tests, the pure `thrust_torque` + model rig in `propeller.rs`'s)

In `crates/ofs-electrical/tests/esc_motor.rs`:

```rust
#[test]
fn a_half_scale_motor_makes_half_the_torque_while_current_limited() {
    // During the 40 A clamp the torque is ke·I_scaled exactly, so omega integrates at exactly half
    // the rate and the published current is half the clamp. (Steady state with no prop load would
    // hide the scale — the no-load current is scale-independent — so the transient is the pin.)
    let full = sim(16.0, 1.0, 1.0);
    for _ in 0..400 {
        full.step().unwrap(); // 50 ms, still on the clamp
    }
    let half = sim(16.0, 1.0, 1.0);
    half.bus_mut().set(half.bus_mut().signal::<f64>(&names::fault_motor_scale(0)), 0.5);
    for _ in 0..400 {
        half.step().unwrap();
    }
    assert_eq!(get(&half, &names::motor_current(0)), 20.0, "the 40 A clamp scales to 20 A");
    let wf = get(&full, &names::motor_omega(0));
    let wh = get(&half, &names::motor_omega(0));
    assert!(((wh - wf * 0.5) / wf).abs() < 0.02, "torque halves from standstill: {wh} vs {wf}/2");
}

#[test]
fn a_dead_motor_draws_no_current_and_coasts_down() {
    let mut s = sim(10.0, 1.0, 1.0);
    s.run_for(0.5).unwrap(); // spinning
    s.bus_mut().set(s.bus_mut().signal::<f64>(&names::fault_motor_scale(0)), 0.0);
    s.run_for(0.05).unwrap();
    assert_eq!(get(&s, &names::motor_current(0)), 0.0, "scale 0: no current");
    let omega_now = get(&s, &names::motor_omega(0));
    assert!(omega_now > 0.0, "the rotor coasts down through drag, it does not stop dead");
    s.run_for(2.0).unwrap();
    assert_eq!(get(&s, &names::motor_omega(0)), 0.0, "drag stops it");
    let theta = get(&s, &names::motor_theta(0));
    s.run_for(0.1).unwrap();
    assert_eq!(get(&s, &names::motor_theta(0)), theta, "a stopped motor's phase is frozen");
}
```

(Signal registration note: `EscMotor::new` registers the fault signal, so `set` must come after construction — same order as the battery harness's `load()`. If the file lacks a `close`-style helper, write assertions with explicit epsilons as above.)

In `crates/ofs-physics/tests/propeller.rs`:

```rust
#[test]
fn the_thrust_scale_fault_multiplies_both_outputs() {
    let p = prop(vec![(0.0, 0.11)], vec![(0.0, 0.045)]);
    let omega = 24_000.0 * 2.0 * PI / 60.0;
    let (t, q) = thrust_torque(&p, omega);
    let mut bus = Bus::new();
    let model = Propeller::new(0, p, &mut bus);
    bus.set(bus.signal::<f64>(&names::fault_prop_thrust_scale(0)), 0.5);
    bus.set(bus.signal::<f64>(&names::motor_omega(0)), omega);
    model.step(&ofs_core::StepCtx { tick: 0, time_s: 0.0, dt_s: 1.0 / 8000.0 }, &mut bus).unwrap();
    close_thrust(bus.get(bus.signal::<f64>(&names::prop_thrust(0))), t * 0.5);
    close_thrust(bus.get(bus.signal::<f64>(&names::prop_torque(0))), q * 0.5);
}
```

(with a local two-arg closeness helper matching the file's assertion style; extend the existing `model_reads_rotor_speed_and_writes_thrust_and_torque` test's rig shape.)

In `crates/ofs-sim/src/vehicle.rs`'s tests (the clear-restores pin; the honest digest-rejoin pin):

```rust
#[test]
fn clearing_prop_damage_restores_the_part_imbalance() {
    // A quad whose prop part carries imbalance (a temp part file, the M4a helper pattern).
    let mut cfg = shipped_quad();
    cfg.props[0].imbalance_kgm = 2.0e-4;
    let mut vehicle = open_loop(&cfg, WorldConfig::open_field());
    vehicle.run_for(0.01).unwrap();
    assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 2.0e-4, "the part data starts");
    vehicle.set_fault(Fault::PropDamage { prop: 0, thrust_scale: 0.9, imbalance_kgm: 5.0e-4 }, true).unwrap();
    assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 5.0e-4);
    vehicle.clear_faults();
    assert_eq!(vehicle.scheduler.bus().get(vehicle.h.prop_imbalance[0]), 2.0e-4, "the part baseline is restored");
    assert_eq!(vehicle.scheduler.bus().get(vehicle.h.fault_prop_thrust_scale[0]), 1.0);
}

#[test]
fn clear_faults_returns_every_fault_signal_to_its_identity() {
    // The honest re-join pin: a physically diverged trajectory cannot re-join a clean run's
    // digest, so the pin is exhaustive over the fault signals instead.
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.set_fault(Fault::MotorFailure { motor: 1, scale: 0.0 }, true).unwrap();
    vehicle.set_fault(Fault::RadioLoss { fraction: 0.7 }, true).unwrap();
    vehicle.set_fault(Fault::GyroFault { bias_radps: DVec3::new(0.1, 0.0, 0.0), noise_scale: 2.0 }, true).unwrap();
    vehicle.set_fault(Fault::WeakCell { cell: 3, r_offset_ohm: 0.01, v_offset_v: 0.1 }, true).unwrap();
    vehicle.clear_faults();
    let b = vehicle.scheduler.bus();
    for i in 0..4 {
        assert_eq!(b.get(vehicle.h.fault_motor_scale[i]), 1.0);
        assert_eq!(b.get(vehicle.h.fault_prop_thrust_scale[i]), 1.0);
    }
    assert_eq!(b.get(vehicle.h.fault_radio_loss_fraction), 0.0);
    assert_eq!(b.get(vehicle.h.fault_burst_on), 0.0);
    assert_eq!(b.get(vehicle.h.fault_imu_gyro_bias), DVec3::ZERO);
    assert_eq!(b.get(vehicle.h.fault_imu_noise_scale), 1.0);
    assert_eq!(b.get(vehicle.h.fault_battery_weak_cell), 0.0);
    assert_eq!(b.get(vehicle.h.fault_radio_loss), 0.0);
    assert!(vehicle.active_fault_names().is_empty());
}
```

- [ ] **Step 2: Verify fail, implement the missing pieces** — Task 1 landed the seams and Task 3 the writes; expect no new model logic. If `theta` freezing on a dead motor needs a guard (scale 0 → omega decays → theta keeps integrating with the decaying omega, which is correct — no guard), say so in the report.

- [ ] **Step 3: Run, clippy, commit**

```bash
git add crates/
git commit -m "feat(sim): the powertrain faults - motor failure scales torque and current, prop damage scales thrust with imbalance"
```

---

### Task 6: WeakCell, GyroFault, VtxInterference — the pack, the gyro, the picture

**Files:**
- Test: `crates/ofs-electrical/tests/` (extend), `crates/ofs-sensors/tests/` (extend), `crates/ofs-video/tests/link.rs` (extend), `crates/ofs-sim/src/vehicle.rs` tests (cell-voltage synthesis)

**Interfaces:**
- Consumes: Task 1's battery/IMU seams and the `VtxInterferenceHandle`; Task 3's writes and the `battery_cell_voltages` synthesis.
- Produces: nothing new — behaviour only.

- [ ] **Step 1: Write the failing tests**

In `crates/ofs-electrical/tests/battery.rs` (its `sim(params())`/`load`/`get` harness):

```rust
#[test]
fn a_weak_cell_deepens_the_sag_exactly_by_its_offsets() {
    let clean = sim(params());
    load(&mut clean, 10.0);
    clean.step().unwrap();
    let weak = sim(params());
    weak.bus_mut().set(weak.bus_mut().signal::<f64>(names::FAULT_BATTERY_R_OFFSET_OHM), 0.02);
    weak.bus_mut().set(weak.bus_mut().signal::<f64>(names::FAULT_BATTERY_V_OFFSET_V), 0.1);
    weak.bus_mut().set(weak.bus_mut().signal::<f64>(names::FAULT_BATTERY_WEAK_CELL), 1.0);
    load(&mut weak, 10.0);
    weak.step().unwrap();
    let drop = get(&clean, names::BATTERY_VOLTAGE) - get(&weak, names::BATTERY_VOLTAGE);
    assert!((drop - (10.0 * 0.02 + 0.1)).abs() < 1e-9, "sag deepens by I·Δr + Δv, got {drop}");
    // The Thevenin state is untouched: the same SoC and consumed charge.
    assert_eq!(get(&weak, names::BATTERY_SOC), get(&clean, names::BATTERY_SOC));
}
```

In `crates/ofs-sensors/tests/vibration.rs` (Task 2's file; the `run`-style harness, but collecting samples):

```rust
#[test]
fn the_gyro_fault_adds_its_bias_exactly() {
    let mut bus = Bus::new();
    bus.set(bus.signal::<DVec3>(names::FAULT_IMU_GYRO_BIAS), DVec3::new(0.1, 0.0, 0.0));
    let rate = bus.signal::<DVec3>(names::BODY_RATE_FRD);
    bus.set(rate, DVec3::ZERO);
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let accel_ned = bus.signal::<DVec3>(names::BODY_ACCEL_NED);
    bus.set(accel_ned, DVec3::new(0.0, 0.0, -ofs_core::consts::GRAVITY_MPS2));
    let mut imu = Imu::new(params(None), 7, &mut bus);
    imu.step(&ofs_core::StepCtx { tick: 0, time_s: 0.0, dt_s: DT }, &mut bus).unwrap();
    let gyro = bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap());
    assert!((gyro.x - 0.1).abs() < 1e-12, "{gyro:?}");
    assert_eq!(gyro.y, 0.0);
    assert_eq!(gyro.z, 0.0);
}

#[test]
fn the_gyro_fault_scales_the_noise_std_exactly() {
    // Same seed, std 0.002: scale 3.0 triples every sample, so the sample std is exactly 3x.
    fn sampled_std(noise_scale: f64) -> f64 {
        let mut bus = Bus::new();
        bus.set(bus.signal::<f64>(names::FAULT_IMU_NOISE_SCALE), noise_scale);
        let p = ImuParams {
            gyro_noise_std_radps: 0.002,
            gyro_bias_radps: DVec3::ZERO,
            accel_noise_std_mps2: 0.0,
            accel_bias_mps2: DVec3::ZERO,
            vibration: None,
        };
        let mut imu = Imu::new(p, 7, &mut bus);
        let mut samples = Vec::with_capacity(1000);
        for k in 0..1000 {
            imu.step(&ofs_core::StepCtx { tick: k, time_s: k as f64 * DT, dt_s: DT }, &mut bus).unwrap();
            samples.push(bus.get(bus.lookup::<DVec3>(names::IMU_GYRO).unwrap()).x);
        }
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        (samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }
    let ratio = sampled_std(3.0) / sampled_std(1.0);
    assert!((ratio - 3.0).abs() < 1e-9, "the same samples scaled: {ratio}");
}
```

(`body.rate_frd_radps` is a `DVec3` signal — `bus.signal::<DVec3>(names::BODY_RATE_FRD)` then `bus.set(.., DVec3::ZERO)`; the plan's line above compresses that — write both statements properly. The noise run has no bias and a level body, so gyro.x is pure noise; the exact-3x ratio holds because the same seed produces the same sample sequence.)

In `crates/ofs-video/tests/link.rs` (its rig style — `params` built the way the neighbouring tests build theirs, `VideoLink::new(params, divisor, seed, handle, bus)` with Task 1's extra argument). Set the emitter **before** the first run so the per-frequency cache computes with it present; baseline from a second scheduler with the handle cleared:

```rust
#[test]
fn the_injected_emitter_degrades_the_picture() {
    let params = || /* the file's existing near-pilot params, as in the clean-picture tests */;
    let noisy = run_link(Some(emitter()), params());
    let clean = run_link(None, params());
    assert!(noisy.1 > clean.1 + 3.0, "interference {} -> {}", clean.1, noisy.1);
    assert!((clean.0 - noisy.0 - (noisy.1 - clean.1)).abs() < 0.1, "SNR falls by exactly the interference rise");
}

/// Builds the link, runs 0.2 s, returns (snr_db, interference_dbm).
fn run_link(emitter: Option<Emitter>, params: ofs_video::link::LinkParams) -> (f64, f64) {
    let handle = VtxInterferenceHandle::default();
    handle.set(emitter);
    let mut bus = Bus::new();
    let link = VideoLink::new(params, 160, 7, handle, &mut bus);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(link));
    s.run_for(0.2).unwrap();
    (get(&s, names::VIDEO_SNR), get(&s, names::VIDEO_INTERFERENCE))
}

fn emitter() -> Emitter {
    Emitter {
        position: DVec3::new(5.0, 0.0, -1.7),
        freq_mhz: 5700.0,
        power_mw: 500.0,
        antenna: Antenna { kind: AntennaKind::Omni, gain_dbi: 2.0, polarization: Polarization::Rhcp, axis: DVec3::NEG_Z },
    }
}
```

(Adapt the params construction to whatever helper the file's clean-picture tests use; if that file has its own `get`-by-name helper reuse it, else write the three-line lookup like the elrs harness's. The assertions are the contract: interference rises by more than 3 dB and SNR falls by exactly that rise.)

In `crates/ofs-sim/src/vehicle.rs`'s tests:

```rust
#[test]
fn cell_voltages_synthesize_equal_shares_and_the_weak_delta() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.run_for(0.1).unwrap();
    let clean = vehicle.state();
    assert_eq!(clean.battery_cell_voltages.len(), 6);
    let share = clean.battery_voltage_v / 6.0;
    assert!(clean.battery_cell_voltages.iter().all(|c| (c - share).abs() < 1e-9));
    vehicle.set_fault(Fault::WeakCell { cell: 0, r_offset_ohm: 0.02, v_offset_v: 0.1 }, true).unwrap();
    let weak = vehicle.state();
    assert!(weak.battery_cell_voltages[0] < share - 0.05, "cell 0 carries the delta: {:?}", weak.battery_cell_voltages);
    assert!((weak.battery_cell_voltages[3] - share).abs() < 1e-9, "the healthy cells are unchanged");
    vehicle.clear_faults();
    let back = vehicle.state();
    assert!(back.battery_cell_voltages.iter().all(|c| (c - share).abs() < 1e-9));
}

#[test]
fn the_injected_emitter_reaches_the_state_snapshot() {
    let mut vehicle = open_loop(&shipped_quad(), WorldConfig::open_field());
    vehicle.run_for(0.2).unwrap();
    let clean = vehicle.state().video;
    vehicle.set_fault(Fault::VtxInterference { freq_mhz: 5700.0, power_mw: 500.0, position_ned_m: DVec3::new(5.0, 0.0, -1.7) }, true).unwrap();
    vehicle.run_for(0.2).unwrap();
    let noisy = vehicle.state().video;
    assert!(noisy.interference_dbm > clean.interference_dbm + 3.0, "{:?} -> {:?}", clean.interference_dbm, noisy.interference_dbm);
    assert!(noisy.snr_db < clean.snr_db);
    vehicle.clear_faults();
    vehicle.run_for(0.2).unwrap();
    let back = vehicle.state().video;
    assert!((back.interference_dbm - clean.interference_dbm).abs() < 0.5, "clears back: {:?}", back.interference_dbm);
}
```

(The vehicle-level interference test exercises the same link through the real session path — it is the one that catches a handle that never reaches the model. The state's `battery_cell_voltages` needs `VehicleState` + the proto `State` mapping from Task 3.)

- [ ] **Step 2: Verify fail, implement gaps** — Task 1's seams + Task 3's writes should carry these; the video test may reveal the handle append must respect the per-frequency cache invalidation (the handle only changes outside ticks, so no invalidation is needed — if the test says otherwise, fix and report).

- [ ] **Step 3: Run everything, clippy, commit**

```bash
git add crates/
git commit -m "feat(sim): the pack, gyro and video faults - weak cell droop, gyro bias and noise, a runtime interference emitter"
```

---

### Task 7: Live SITL tests — the faults through real Betaflight

**Files:**
- Create: `python/tests/msp_status.py` (the decode helper), `python/tests/test_sitl_faults.py`
- Modify: `.github/workflows/ci.yml` (the sitl job's pytest list gains `test_sitl_faults.py`)
- Test: the new files themselves

**Interfaces:**
- Consumes: Task 3's python dataclasses; the shipped quad; the WSL live recipe.
- Produces: `msp_status.FailsafeWatch` (or function set) — `arming_disable_flags(request) -> int` walking the pinned MSP_STATUS layout; `FAILSAFE = 1 << 1`, `RX_FAILSAFE = 1 << 2` constants with their provenance comment (`runtime_config.h:42-44`, `failsafe.c:195/490`).

- [ ] **Step 1: Write `python/tests/msp_status.py`** — the MSP_STATUS client and decode, self-contained (modeled on `test_sitl_telemetry.py`'s `_msp_requester`; do not refactor the other test files):

```python
"""MSP_STATUS decode for the fault tests: Betaflight's own failsafe flags (m3c debt closed).

A real RX-loss failsafe never sets the BOXFAILSAFE pilot-switch bits (failsafe.c only reads
IS_RC_MODE_ACTIVE(BOXFAILSAFE)), so the decoded observable is the arming-disable flags:
RX_FAILSAFE (1 << 2) from stage 1 on, FAILSAFE (1 << 1) once stage 2 engages
(Betaflight 2026.6.2: runtime_config.h:42-44; set at failsafe.c:195/214/231 and :490).
"""
import socket

MSP_STATUS = 101
ARMING_DISABLED_FAILSAFE = 1 << 1
ARMING_DISABLED_RX_FAILSAFE = 1 << 2


class StatusClient:
    """One MSP_STATUS channel over the Configurator TCP port, pumped by simulated time."""

    def __init__(self, sim, timeout_s=2.0):
        port = int(sim.configurator_address.rsplit(":", 1)[1])
        self._sock = socket.create_connection(("127.0.0.1", port), timeout=timeout_s)
        self._sock.settimeout(timeout_s)
        self._sim = sim

    def _exchange(self, cmd):
        ck = cmd
        self._sock.sendall(b"$M<" + bytes([0, cmd]) + bytes([ck]))
        while True:
            self._sim.run(0.02)  # MSP is serviced only while simulated time advances
            b = self._sock.recv(4096)
            if b"$M>" in b:
                i = b.index(b"$M>")
                n, got = b[i + 3], b[i + 4]
                if got == cmd:
                    return b[i + 5:i + 5 + n]

    def arming_disable_flags(self):
        """One MSP_STATUS exchange: walks the pinned payload to the u32 LE arming-disable flags.

        Layout (Betaflight msp.c, case MSP_STATUS): [0..2] u16 cycle time, [2..4] u16 i2c errors,
        [4..6] u16 sensors, [6..10] u32 LE flight-mode flags, [10] u8 pid profile, [11..13] u16
        system load, [13..15] u16 gyro cycle time, [15] u8 extended-box byte count + that many
        bytes, then u8 arming-disable flag count, then the u32 LE flags.
        """
        payload = self._exchange(MSP_STATUS)
        i = 15
        i += payload[i] + 1  # skip the extended flight-mode-flag bytes
        count = payload[i]
        i += 1
        return int.from_bytes(payload[i:i + count], "little")

    def close(self):
        self._sock.close()
```

- [ ] **Step 2: Write `python/tests/test_sitl_faults.py`** (follow `test_sitl_radio.py`'s structure — the `sim` fixture, the ARM aux convention, the timing style; the plan's assertions and fault choices are the contract, the plumbing follows the existing tests):

```python
"""The fault catalog through real Betaflight SITL (spec §8). Needs OFS_SITL_LAUNCH."""
import ofs
from ofs.faults import BurstLoss, MotorFailure, VtxInterference, WeakCell

from conftest import QUAD
from msp_status import ARMING_DISABLED_FAILSAFE, ARMING_DISABLED_RX_FAILSAFE, StatusClient

ARM = ...  # the suite's existing arm-aux convention, copied from test_sitl_radio.py


def _armed_and_hovering(sim):
    sim.load(QUAD, seed=1)
    sim.run(4.0)
    sim.set_sticks(throttle=0.0, aux=ARM)
    sim.run(1.0)
    sim.set_sticks(throttle=0.55, aux=ARM)
    sim.run(2.0)


def test_motor_failure_takes_the_quad_down(sim):
    _armed_and_hovering(sim)
    healthy = sim.state()
    sim.inject(MotorFailure(motor=0, scale=0.0))
    sim.run(1.0)
    dead = sim.state()
    assert dead.motor_rpm[0] < healthy.motor_rpm[0] * 0.5
    assert dead.motor_cmd[0] > healthy.motor_cmd[0], "Betaflight compensates"
    assert abs(dead.euler_deg()[0]) + abs(dead.euler_deg()[1]) > 5.0, "the quad tilts off the lost corner"


def test_burst_loss_failsafes_and_betaflight_flags_it(sim):
    _armed_and_hovering(sim)
    status = StatusClient(sim)
    sim.inject(BurstLoss(on_ms=2500, off_ms=1000))
    sim.run(3.5)  # past stage 1 (Betaflight's 1 s default) into stage 2
    flags = status.arming_disable_flags()
    assert flags & ARMING_DISABLED_RX_FAILSAFE, hex(flags)
    assert flags & ARMING_DISABLED_FAILSAFE, hex(flags)
    sim.clear_faults()
    sim.run(2.0)
    flags = status.arming_disable_flags()
    assert not flags & ARMING_DISABLED_FAILSAFE, f"recovered: {flags:#x}"
    status.close()


def test_weak_cell_reads_through_betaflight_telemetry(sim):
    _armed_and_hovering(sim)
    healthy = sim.state()
    sim.inject(WeakCell(cell=0, r_offset_ohm=0.02, v_offset_v=0.1))
    sim.run(0.5)
    weak = sim.state()
    cells = weak.battery_cell_voltages
    assert len(cells) == 6
    assert cells[0] < min(cells[1:]), "cell 0 carries the delta"
    assert weak.fc_telemetry.battery_v < healthy.fc_telemetry.battery_v


def test_vtx_interference_degrades_the_picture(sim):
    _armed_and_hovering(sim)
    clean = sim.state().video
    sim.inject(VtxInterference(freq_mhz=5700.0, power_mw=500.0, position_ned_m=(10.0, 0.0, -1.5)))
    sim.run(0.5)
    noisy = sim.state().video
    assert noisy.interference_dbm > clean.interference_dbm
    assert noisy.snr_db < clean.snr_db
```

(`ARM` is whatever `test_sitl_radio.py` passes — copy its exact aux tuple and throttle values; if that test defines arming differently, match it. The failsafe timings assume Betaflight's defaults, which the shipped diff does not override — verified: no failsafe lines in the diff.)

- [ ] **Step 3: Run the live suite in WSL** (the recipe; a Linux-built `ofs-sim` from this tree) — `uv run --with grpcio,protobuf,pytest pytest python/tests/test_sitl_faults.py -v` — Expected: 4 PASS. Then the full python suite.

- [ ] **Step 4: CI** — add `python/tests/test_sitl_faults.py` to the sitl job's named list (`.github/workflows/ci.yml`; the m3e debt records that the list is explicit). Do not add `test_sitl_link.py` (stays carried debt).

- [ ] **Step 5: Commit**

```bash
git add python/tests/msp_status.py python/tests/test_sitl_faults.py .github/workflows/ci.yml
git commit -m "test(sitl): the fault catalog live - motor failure, flagged failsafe burst, weak cell, video interference"
```

---

### Task 8: Docs — the faults research note, carried debt, README

**Files:**
- Create: `docs/research/faults.md`, `docs/superpowers/m4b-carried-debt.md`
- Modify: `README.md`, `docs/dev-setup.md` (one paragraph each)

**Interfaces:** none (documents only).

- [ ] **Step 1: `docs/research/faults.md`** — per fault: the mechanism (one paragraph, from this plan's rulings), every named constant, and its estimate status. The constants: `RadioLoss.fraction` (user input), the burst window (user input), the interference emitter's default antenna model (2 dBi RHCP omni — the same default as the VTX antenna, an estimate), the vibration model's two formulas (Ruling 8 — rigid-body transmission, below-resonance amplitude, "every constant is an estimate; none is measured", the m3c/m3b debt phrasing), the weak-cell synthesis formula (Ruling 7). State the m3c-debt close: the failsafe decode is Betaflight's own flags, pinned bytes in `python/tests/msp_status.py`'s docstring.

- [ ] **Step 2: `docs/superpowers/m4b-carried-debt.md`** — following `m4a-carried-debt.md`'s structure:

```markdown
# M4b carried debt

## Deferred features
- **Windmilling drag on a dead motor** — a scale-0 motor freewheels through prop drag only.
- **Per-cell battery model** — WeakCell is additive on the pack Thevenin; capacity fade stays out.
- **Vibration transmissibility** — rigid-body, below-resonance only; no structural model.
- **RadioLoss on the downlink** — only the uplink fraction is modelled; the downlink keeps the
  link model's own error rate (the telemetry slot ratio is unaffected).
- **Vibration on mixed pole-pair open-loop builds** — theta is per-motor and honest, but the
  rate disturbance ignores gyroscopic coupling (pre-existing physics simplification).
- **`test_sitl_link.py` is still not in the sitl job's list** (inherited m3e debt).

## Rulings worth remembering
- Fault randomness comes from a dedicated `elrs-faults` RNG stream: fault-active runs differ
  from fault-free ones only in the loss decisions; the model's own stream never shifts.
- The failsafe decode reads arming-disable flags (RX_FAILSAFE 1<<2, FAILSAFE 1<<1), not the
  BOXFAILSAFE mode bit — a real RX-loss failsafe never sets the pilot-switch path.
- `fault.battery.weak_cell` carries cell index + 1 (0 = none) so the synthesis can find it.
```

- [ ] **Step 3: README and dev-setup** — README's fault mention (the `ofs.faults` example) grows from `RadioLinkLoss` to the catalog: list the eight with one-line meanings, keep the §6.2-style example working verbatim. dev-setup.md gains one sentence in its schema/API section: protocol 8 and where the faults live.

- [ ] **Step 4: Full verification and commit**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets --locked -- -D warnings`, the Godot headless suite (`scripts/run-godot-tests.sh` — the event-kind addition touches its HUD path), and the full python live suite in WSL.

```bash
git add README.md docs/
git commit -m "docs: M4b - the fault catalog documented, its estimates recorded, the m3c failsafe-decode debt closed"
```
