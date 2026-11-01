# Crash damage: degrade and repair (#157, #29 part 2 of 2; epic #160)

Simulator work: `ofs-physics` (contact reporting), `ofs-sim` (the damage model, protocol 17), the Python and Rust
clients and the Godot bridge's telemetry keys. The brief is issue #157; the binding contracts are the epic #160
body ("damage rides the existing fault seams (`PropDamage`, `MotorFailure`), never a parallel damage system, with
repair a workbench action and `Load` a reset; damage constants are estimates with provenance"), the M4b fault
catalog (`docs/research/faults.md`; the seams at `crates/ofs-sim/src/vehicle.rs:212-256`), #156's collision
machinery (`crates/ofs-physics/src/collision.rs`), and the workbench spec's DC8 repair seam
(`docs/superpowers/specs/2026-10-31-drone-workbench-design.md:114-122`). No design conversation took place, so
every ruling below is the author's call and open to change on review. Written 2026-10-31 against `main` at
`bd4cc20` (protocol 16); every executor re-verifies the anchors before touching code.

## Why

A crash costs nothing today: `docs/research/collision.md` closes with "nothing is damaged". The impulse that
stops the quad is computed per contact (`collision.rs:158`, `j = -(1+e)·v_n / k`) and then thrown away; the
collision event toasts and Betaflight wrestles the tumble, but the airframe is factory-new forever. Issue #157
asks for the missing consequence: thresholds per part kind turn a crash impulse into accrued damage that rides
the existing fault seams — a bent prop loses thrust and shakes the gyro through the IMU's analytic imbalance
term (`crates/ofs-sensors/src/imu.rs:126-172`, `F = imbalance·ω²`), a motor hit surfaces through `MotorFailure`,
a dislodged battery through `WeakCell` — shown in what the pilot and the FC already see (RPM sag, gyro noise,
voltage sag), never a damage HUD.

## Goals

1. **Impulse → damage**: every sphere contact's normal impulse, reported per contact, accrues per-part damage
   above provenance-marked thresholds — unit-tested from scripted crash impulses.
2. **Degradation is physical and rides the seams**: bent-prop thrust loss and imbalance (the gyro visibly shakes
   in a recorded IMU signal), motor torque loss, battery series resistance — through the existing `fault.*` and
   `prop.{i}.imbalance_kgm` signals, composed with script-injected faults, never a parallel system.
3. **Repair is an RPC**: protocol 17 carries `State.damage` (the per-part surface the workbench's rows will
   show) and a part-selective `Repair`; `clear_faults` and `Load` also reset damage. The round trip
   crash → fly worse → repair → fly right is an integration test, today, from Python.
4. **The follow-on is ruled, not built**: the workbench UI (condition display + Repair on DC8's per-kind rows)
   is a separate task that consumes this seam after #128 executes.

## Non-goals

- No new bus signal, no new `Fault` variant, no `[damage]` quad-file section (thresholds are model constants,
  estimates with provenance, like the collision constants in `docs/research/collision.md`'s table); no damage
  HUD, no condition readout in the goggles; no `GyroFault` from crashes (a bent prop shakes the airframe, the
  sensor itself stays honest).
- No audio change: PR #162's thud (`d07e0fb`, one per collision event) already sounds the hit, and a bent
  prop's acoustic character is not modelled (#158's synthesis reads rpm and command, unchanged).
- No persistence beyond the session: damage lives in the vehicle, `Load` rebuilds the vehicle, nothing survives
  a reload, no field in the assembly file (the workbench spec's DC8 rules condition session state).
- No Godot repair UI and no worker call plumbing for it (the follow-on); no visual part damage; no damage from
  landing-point spring contacts or friction impulses.

## Design calls

**DC1. Damage is a per-part level in the vehicle, expressed only through the existing fault seams.** A new pure
module `crates/ofs-sim/src/damage.rs` holds `PartDamage { props: Vec<f64>, motors: Vec<f64>, battery: f64 }`
(levels 0..1, one per motor index, saturated) and the constants and curves below. The vehicle owns the state and
writes the same signals `Vehicle::set_fault` writes (`crates/ofs-sim/src/vehicle.rs:1396-1407`):
`fault.prop.{i}.thrust_scale`, `prop.{i}.imbalance_kgm` (on top of the part baseline,
`vehicle.rs:1401`), `fault.motor.{i}.scale`, and `fault.battery.{weak_cell,r_offset,v_offset}`. The
propeller, ESC/motor, battery and IMU models change not at all — they already read these seams.

**DC2. The trigger is the per-contact normal impulse.** `collision::resolve` already computes each sphere
contact's normal impulse `j`; it will collect `(centre_frd_m, impulse_n_s)` for every contact with `j > 0` and
hand them to `RigidBody`, which pushes them on a `ContactFeed` shared handle (`Arc<Mutex<Vec<ContactImpulse>>>`,
the `RaceHandle`/vtx-emitter pattern) returned beside the model at build. The vehicle drains the feed once per
tick inside `step_ticks`'s existing per-tick loop (`vehicle.rs:1618-1626`, next to `apply_due`). Mechanism in
`ofs-physics` (report the impulse), policy in `ofs-sim` (threshold it). Resting contacts cannot creep: the
shipped quad's weight is ~8e-4 N·s per tick against a 1.5 N·s floor. The event machinery
(`EVENT_MIN_SPEED_MPS`, rearm, messages) is untouched; landing-point spring touches carry no impulse and never
damage; friction impulses never damage (a scrape is not a crush).

**DC3. Impulse → part by body-frame geometry.** A contact belongs to the prop of the nearest motor
(`frame.motor_positions_frd_m`) when its sphere centre lies within that motor's prop tip circle grown by the
sphere radius plus a 1 cm margin — this covers both the generated spheres (six per prop tip circle,
`crates/ofs-config/src/lib.rs:1062-1074`) and any custom `spheres_frd_m`. A prop-region contact with
`j` at or above the motor threshold also accrues that motor (the strike went through the prop into the
bearing/bell). A contact outside every disc is a chassis hit and accrues the battery above its threshold. The
mapping needs the geometry the vehicle already holds at build; no new config.

**DC4. Accrual: linear above a threshold, saturated at 1.** `level += (j − T) / (J_full − T)` for `j ≥ T`, per
contact, per part, capped at 1.0; repeated crashes stack. Thresholds and full-damage impulses (estimates, the
provenance table below): prop bends from 1.5 N·s and is fully bent at 5 N·s; the motor damages from 4 N·s and is
fully damaged at 10 N·s; the battery dislodges from 6 N·s and is fully dislodged at 15 N·s. Calibration sanity,
from the impulse arithmetic in `docs/research/collision.md`: a square-on 10 m/s wall stop puts ~8.5 N·s through
the belly sphere (battery level ~0.28), a 30 m/s post strike on a prop tip ~8 N·s (prop capped, motor ~0.6), a
1.5 m/s botched landing on a prop sphere ~0.4 N·s (nothing). A 0.65 kg quad at 8 m/s, the shipped restitution
0.3: `j ≈ 1.3 · m_eff · v`.

**DC5. Degradation curves (estimates).** Level 1 means: prop `thrust_scale = 0.5` (a bent prop keeps half its
thrust; a fully shattered prop is a scenario `PropDamage`, not crash accrual) and `imbalance_kgm = 2.0e-5` on
top of the part baseline (a chipped 5-inch tip: ~0.3 g at ~50 mm); motor `scale = 0.2` floor (crash damage
never fully kills a motor — `MotorFailure { scale: 0 }` stays a scenario); battery `r_offset = 0.03 Ω` and
`v_offset = 0.1 V` on cell 0 (`WeakCell`'s seam holds one cell; a real dislodged pack sags every cell — carried
debt, see the open question). All interpolate linearly in the level. Every constant is an estimate with
provenance marked in the code's doc comments and the constants table; the validation path is `docs/validation.md`
(thrust-stand and blackbox submissions; none exists for crash damage yet — the table says so).

| Constant | Value | Provenance |
|---|---|---|
| Prop bend threshold / full damage | 1.5 / 5.0 N·s | estimate (sized from the impulse arithmetic above) |
| Motor damage threshold / full | 4.0 / 10.0 N·s | estimate |
| Battery dislodge threshold / full | 6.0 / 15.0 N·s | estimate |
| Max prop thrust loss | 0.5 | estimate |
| Max prop imbalance | 2.0e-5 kg·m | estimate (a chipped 5-inch tri-blade tip, ~0.3 g at 50 mm) |
| Max motor scale loss | 0.8 (floor 0.2) | estimate |
| Max battery Δr / Δv | 0.03 Ω / 0.1 V | estimate (connector and lead damage under 30 A) |
| Prop disc membership margin | tip radius + sphere radius + 1 cm | engineering choice |
| Battery fault's cell | 0 | ruling (the `WeakCell` seam names one cell) |

**DC6. The gyro shakes through the existing imbalance term — no new disturbance seam.** Damage raises
`prop.{i}.imbalance_kgm`; the IMU's analytic vibration model (`imu.rs:126-172`) already turns that into a
rotating force `F = m·e·ω²` at the motor's frequency, through the frame's mount if it has one. The test the
issue asks for — "a damaged prop's vibration is visible in a recorded IMU signal" — records
`imu.gyro_frd_radps` (the `mount.rs` digest pattern, plus a recording) and asserts the motor-frequency
content of the damaged run exceeds the healthy run's. `GyroFault` (bias, noise) is a sensor fault and is never
applied by a crash.

**DC7. Composition with script-injected faults.** The vehicle remembers the script's absolute per-part fault
values (`Option` per prop, per motor, and one for the battery; `None` = identity), set by `apply_fault`'s
`PropDamage`/`MotorFailure`/`WeakCell` arms and cleared by deactivation and `clear_faults`. One composer writes
the seams whenever either side changes: thrust scales and motor scale multiply, imbalance and battery offsets
add. A scripted fault therefore survives a crash on the same part instead of being silently overwritten, and
clearing the script fault leaves the crash damage standing. `clear_faults` clears crash damage too — it is the
clean-slate debug call (every signal to its identity, `vehicle.rs:1584-1604`), which makes
`sim.clear_faults()` the scripts' repair until the workbench exists.

**DC8. Determinism: onset is derived, repair is an input.** Damage onset follows from the physics (same inputs
→ same contacts → same impulses → same damage) and is never journaled; applying it inside `step_ticks`'s
per-tick loop keeps a lockstep run the same however its `Run` calls are cut (the `schedule_fault` rule,
`vehicle.rs:1438-1440`). `Repair` is a client input and is journaled (`Action::Repair { props, motors, battery }`
in `crates/ofs-sim/src/journal.rs`, replayed like `Action::Fault`). A run without a damaging contact writes no
seam and is bit-identical to today; every golden (`crates/ofs-sim/tests/golden`, `crates/ofs-radio/tests/golden`)
must still match — re-verify at execution that no golden run crashes hard enough to damage (none is expected
to: the scripted flights land softly).

**DC9. The wire: protocol 17.** From 16 (the worktree holds `bd4cc20`, #169). The open DVR-audio PR #170
touches no proto file (verified: empty diff over `proto/`, `crates/ofs-proto/`, `python/ofs/v1/`), so 17 is
uncontested; an executor finding #170 merged re-checks only the version constant. `State` gains
`repeated double damage_prop = 21`, `repeated double damage_motor = 22`, `double damage_battery = 23` (levels
0..1; flat fields in the `motor_rpm` style — field numbers re-verify at execution). A new
`rpc Repair(RepairRequest) returns (Empty)` beside `ClearFaults`, with
`RepairRequest { repeated uint32 props; repeated uint32 motors; bool battery; }`. **Events: no new kind.**
Damage onset marks the fault active (`active_faults`, `vehicle.rs:1555-1560`), so the session's existing edge
detection raises `FAULT_INJECTED` (`session.rs:498-507`) and repair or `Load` raises `FAULT_CLEARED` — the
pilot sees today's toasts plus the symptoms (RPM sag, gyro noise, voltage sag); per the issue, no damage HUD
and no new message format.

**DC10. Repair is part-selective and validated.** `Vehicle::repair(&mut self, props: &[usize], motors:
&[usize], battery: bool)` clears those parts' levels (rewriting the seams through the composer), refuses
out-of-range indices with `InvalidArgument` exactly as `check_fault` does (`vehicle.rs:1316-1339`), and refuses
an empty repair ("nothing to repair") — implicit repair-all is a footgun; a client repairs everything by listing
every part, which `State.damage`'s lengths make trivial. Persistence is thin: the level lives in the vehicle,
`Load` builds a fresh one (`server.rs:291-364`), and nothing is written anywhere.

**DC11. Clients now; the UI later.** Python: `State.damage` (a frozen `Damage(props, motors, battery)`
dataclass, defaulted like `Race`) and `sim.repair(props=(), motors=(), battery=False)`; the round-trip e2e
uses them. Rust client: `Telemetry.damage_prop/damage_motor/damage_battery` in `crates/ofs-client/src/model.rs`
(from_pb beside `motor_rpm`). Godot bridge: `get_telemetry()` gains the read-only keys `damage_props`,
`damage_motors`, `damage_battery` — the surface the follow-on consumes; no worker command, no UI, no settings
key in this milestone.

**DC12. The follow-on (after #128 executes): condition rows and the Repair action in the workbench.** The
workbench's per-kind part rows (DC8's `row("props")` / `row("motors")` / `row("battery")` seam,
`select_part(kind)`-driven details) show the part's condition from `State.damage` (healthy / bent / dead
wording over the level) and a Repair action that calls the new RPC through the bridge — which is when the
worker gains its repair command; until then `Load` (the menu's Start) is the pilot's reset. That task is
planned when #128's screen exists; this spec's contract to it is exactly `State.damage` + `Repair`.

## What is left to the builder

The crash integration tests' exact scripting (drop heights, stick inputs for a tilted prop strike); the
recorded-IMU assertion's statistic (windowed peak-to-peak vs std at the motor frequency); the toast wording, if
any, beyond the existing fault names.

## Checked by eye (headless suites render nothing)

A crashed quad flying audibly and visibly worse before repair and right after (a human flight through
`godot/`); the workbench rows' condition wording once the follow-on lands (manual QA in #163 per the phase
umbrella).

## Open questions for the maintainer

1. **Pack-wide battery sag.** A dislodged battery sags the whole pack; the `WeakCell` seam the epic binds
   damage to holds exactly one cell (DC5's table records the ruling: cell 0, visible as that cell's droop in
   `State.battery_cell_voltages`). The honest alternative is a tenth fault variant (a pack-level electrical
   fault) — a catalog change the epic's "never a parallel system" does not force but does not forbid either.
2. **Should one catastrophic impulse kill a motor outright?** DC5 floors crash motor damage at `scale = 0.2`;
   a fully dead motor stays a scripted `MotorFailure`. Say the word and the floor drops to 0.
