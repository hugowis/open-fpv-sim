# Rotor dynamics: what the spinning parts do to the airframe

Three effects the rigid-body model of v1 left out, added in M6a. Each is optional part data that is off when absent,
so a part without it behaves exactly as before. **Every constant is an estimate; none is measured.**

## Rotor gyroscopic coupling

A spinning rotor carries angular momentum. When the airframe turns, the rotors' momentum is dragged round with it and
pushes back. `RigidBody` (`crates/ofs-physics/src/rigid_body.rs`) integrates

    I·ẇ = τ − w×(I·w) − w×h − c·w          h = Σₖ spinₖ · Jₖ · ωₖ · ẑ

where `w` is the body rate, `τ` the summed thrust, reaction and contact torque, `c` the frame's angular damping,
`spinₖ` is ±1 (`+1` = clockwise from above, along +Z in FRD), `Jₖ` the rotor's moment of inertia (motor rotor plus
propeller, `AirframeParams.rotor_inertia_kgm2`) and `ωₖ` its **mechanical** speed in rad/s (`motor.{k}.omega_radps`).
`h` is held over the step, like the torque.

The other half of the same physics was already there: a rotor that spins up reacts on the body with `−spin·J·ω̇` about
Z (it is in the yaw reaction torque), which is `−dh/dt`. Together they are the angular-momentum balance of body plus
rotors.

What it does in flight: with the two clockwise and two counter-clockwise rotors at equal speed, `h` cancels and
nothing changes. It matters whenever the speeds differ, which is every manoeuvre (a roll stirs the rotors' pairs
differently) and above all a dead or damaged motor, where one rotor's momentum is no longer cancelled and the quad
precesses.

- **Pole pairs never enter.** `h` uses the mechanical speed; pole pairs only scale the eRPM Betaflight is told, so an
  open-loop quad with mixed pole pairs is unaffected.
- **Rotor inertia is not folded into the body inertia tensor.** The rotors' inertia about axes other than their spin
  axis is a few parts in a thousand of the frame's.
- The test is the analytic precession: a body rolling at `w_x` with `h` along Z accelerates in pitch at `w_x·h/I_y`.
  The torque does no work (`w·(w×h) = 0`), which a second test pins by energy conservation.

| Constant | Value | Provenance |
|---|---|---|
| `Jₖ` | the motor part's `rotor_inertia_kgm2` plus the prop part's `inertia_kgm2` | estimated (`docs/research/component-library.md`) |

## Windmilling

A rotor with no drive current still turns when air comes up through its disc (the aircraft descending), and past some
descent speed that air spins it up instead of braking it. The propeller (`crates/ofs-physics/src/propeller.rs`) has
a torque coefficient `Cp` that is a function of rotor speed only; M6a makes it depend on the **advance ratio**
`J = V/(n·D)`, `V` the inflow through the disc, `n` rev/s, `D` the diameter:

    Cp_eff = Cp · (1 − J/j0)           for V > 0 and j0 > 0

`j0` (`prop.windmill_j0`) is the advance ratio at which the torque vanishes (autorotation); beyond it the torque
changes sign and drives the rotor. The torque is evaluated multiplied out,

    Q = Cp · ρ · D⁴ · n · (D·n − V/j0) / 2π

so nothing divides by `n`: it is finite and falls to zero as the rotor stops. Thrust is **not** touched; a climb
(`V ≤ 0`) is not touched; `j0 = 0` (the default for a part that does not set it) is the old model bit for bit.

The inflow is `prop.{i}.inflow_mps`, published by the rigid body: the mount's velocity along body +Z,
`v_body.z + (rate × position).z`, positive when the mount moves down. The propeller runs before the body within a
tick, so it reads the previous tick's value. The prop axis is assumed to be body Z.

The effect applies to every rotor, powered or not, so the rotor speed Betaflight reads (eRPM, ESC telemetry) moves
continuously when a motor dies in a descent. In powered flight it lowers the torque in a descent, which is real.

**Steady state.** With no drive current the motor's only other torque is its friction `F = Ke·I0` (the model's
no-load current), so the rotor settles where `Cp·ρ·D⁴·n·(V/j0 − D·n)/2π = F`, i.e. the larger root of

    D·n² − (V/j0)·n + κ = 0,        κ = 2π·F / (ρ·D⁴·Cp)

(`crates/ofs-sim/tests/windmill.rs` checks the speed against this to 1 %). It has no root when `(V/j0)² < 4Dκ`:
below that descent speed the rotor coasts to a stop. Because the model's friction is the whole no-load current, the
shipped motor needs a very fast descent to windmill; a part with a smaller no-load current windmills sooner.

**Limitations.** A rotor stopped at exactly 0 rad/s stays stopped: the drive is proportional to `n`, so there is
no start-up torque from rest. `Cp` is not made to depend on `J` in a powered climb, and thrust does not fall with
the advance ratio.

| Constant | Value | Provenance |
|---|---|---|
| `prop.windmill_j0` | 1.0 | estimated: the zero-torque advance ratio of a small propeller, an order of magnitude, not measured |
