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
