# Scripted flight: the open-loop stabilizer and the circuit pilot

A Python script can fly a precise path. Two pieces make that possible without a person: a built-in stabilizer for open-loop
sessions (fast, no Betaflight), and a scripted pilot that flies a closed circuit through the gates of
`worlds/circuit.toml`. The same pilot also flies real Betaflight SITL, and a test asserts that both complete a lap
(`python/tests/test_circuit.py`, `python/tests/test_sitl_circuit.py`).

## The stabilizer

```python
sim.load("quads/opendrone-5f-freestyle.toml", open_loop_fc=True, stabilize=True, world="circuit")
sim.set_sticks(roll=0.2, throttle=0.4)   # leans to 12 degrees and holds it
```

`open_loop_fc=True` alone is unchanged: the throttle goes to every motor and nothing steers. `stabilize=True` adds a minimal
angle-mode controller on top; it needs `open_loop_fc` (otherwise `load` raises `InvalidArgument`). The Rust name is
`FcKind::OpenLoopStabilized`, and a quad file may ask for it with `[fc] kind = "open_loop_stabilized"`.

**It is a test stand-in, not Betaflight. Real tuning needs SITL.** It reads the simulator's truth (the attitude and the body
rates, with no IMU noise and no filtering) and it has no arming, no rates or expo, no I-term, no throttle-PID attenuation, no
anti-gravity, no airmode and no RPM filter. Use it to fly paths fast, in CI and on a machine without WSL; use SITL when the
tune matters. Its gains suit the shipped quad.

### Sticks

The stick meanings are Betaflight's angle mode, so one script can fly both:

| stick | meaning |
|---|---|
| roll `+` | leans right (positive roll) to `stick * 60` degrees |
| pitch `+` | forward: nose down, to `stick * 60` degrees |
| yaw `+` | turns clockwise seen from above at `stick * 200` degrees per second |
| throttle | the collective command, as in the throttle-only stand-in; at or below 0.05 every motor is 0, so a quad on the ground stays put |

### The control law

A cascade, in `crates/ofs-fc/src/stabilizer.rs`: the angle error sets a roll or pitch rate target (`KP_ANGLE = 15` per second,
at most 400 degrees per second); the rate error becomes a motor-command offset (`KP_RATE_ROLL_PITCH = 0.035`,
`KP_RATE_YAW = 0.06`, command per radian per second); a mixer built from the frame's motor positions and spin directions
spreads the three offsets over the motors and clamps each command to [0, 1]. The gains were estimated from the shipped
quad's inertia and thrust slope and tuned against `crates/ofs-sim/tests/stabilizer.rs`: a 0.3 roll stick reaches 18.5 degrees
in half a second and peaks at 20.6; the yaw rate follows its stick within 15 %. The yaw gain needed doubling from its
estimate, because a rate loop with only a P term loses part of the demand to the rotors' drag torque.

### Repro bundles

A bundle of a stabilized session records `stabilize = true` in `bundle.toml` and replays with the stabilizer
(`docs/repro-bundle.md`). A bundle without the key was never stabilized.

## The circuit and the pilot

`worlds/circuit.toml` has four gates, 6 m wide and 4 m tall from the ground up, on a 40 m square flown clockwise seen from above.
Gate0 (the start and finish) at 20 m north is flown eastwards, Gate1 at (40, 20) northwards, Gate2 at (60, 0) westwards and
Gate3 at (40, -20) southwards; home is at the origin. The rules for a pass, a split and a lap are in `docs/race-timing.md`.

`python/examples/circuit.py` flies it:

```python
import importlib.util, ofs
spec = importlib.util.spec_from_file_location("circuit", "python/examples/circuit.py")
circuit = importlib.util.module_from_spec(spec); spec.loader.exec_module(circuit)

with ofs.launch() as sim:
    sim.load("quads/opendrone-5f-freestyle.toml", open_loop_fc=True, stabilize=True, world="worlds/circuit.toml")
    result = circuit.fly_circuit(sim)
    print(result["laps"], result["seconds"], result["events"][-1])
```

The pilot steers with the sticks, like a person: an altitude loop on the throttle, and a position loop that turns the
distance to the next waypoint into a speed (at most 5 m/s) and then a lean (at most 3 m/s^2), holding the heading north so
roll is east and pitch is north. It reads the quad's state, never the gate events, to steer. It takes off from home, passes
within 2.5 m of each waypoint in turn and raises `RuntimeError` naming the waypoint when one is not reached in time. On the
stabilizer a lap takes about 35 s and the whole flight about 51 s of simulated time.

With Betaflight (`fly_circuit(sim, betaflight=True)`, or `python python/examples/circuit.py --betaflight` with
`OFS_SITL_LAUNCH` set) the pilot first sits still for the 4 s boot, arms with the throttle down (AUX1) and selects angle mode (AUX2).
Two things differ from the stabilizer and are tuned per backend in the script (measure a tune's lean yourself: hold a
full stick for 2 s in a hover and read the attitude): the shipped tune leans only about 24 degrees at
a full angle-mode stick (measured: a 0.3 stick leans 7 degrees), not the 60 the stabilizer gives; and the quad has to be
armed with the throttle down before the altitude loop may raise it. A lap takes about 33 s there.

### Tests

- `python/tests/test_circuit.py` (every CI run, no Betaflight): the lap completes on the stabilizer with the expected events
  and no collision; a loop flown too far north misses Gate2 and counts no lap; an unreachable waypoint raises; two runs agree.
- `python/tests/test_sitl_circuit.py` (the `sitl` CI job, or locally with `OFS_SITL_LAUNCH`): the same assertions on real
  Betaflight.

Not covered: an IMU-fed or filtered stabilizer, altitude or position hold inside it, acro/rate mode, and any claim that
its response matches a real Betaflight tune (see issue #51). The game has no `stabilize` option, but a quad file with
`[fc] kind = "open_loop_stabilized"` flies stabilized in Godot just as in Python (the same gRPC path the Rust
stabilizer test drives); there is no Godot-specific test for it.
