# A compass: the magnetometer model and the heading mapping (#31)

Closes #31. No design conversation took place (the brief was to state the calls in the spec), so every ruling below is the
author's call and open to change on review.

## Why

The simulator has an IMU and a barometer and no magnetometer. The issue asks for one: the earth field vector, the body
rotation, noise and hard/soft-iron distortion as data, and "the mapping Betaflight needs".

## What Betaflight SITL accepts (read first, it sets the scope)

The SITL state datagram (`FdmPacket`, 144 bytes) has **no magnetometer field**: gyro, accel, attitude quaternion,
velocity, position, pressure. The SITL build (Betaflight 2026.6.2, `USE_VIRTUAL_MAG`) synthesises the compass itself, in
`sitl.c`, from the attitude quaternion we already send: a fixed earth field of 4096 counts, 60° inclination, 2° east
declination (NWU `(2046.8, -71.5, -3547.2)`), rotated into the body by that quaternion. So:

- Betaflight's compass reads the **true attitude** through a **fixed field**. Noise, hard iron, soft iron, a different
  field or a different declination **cannot reach it** without either lying in the quaternion (which also feeds the
  attitude path) or extending the datagram and patching SITL (a new reserved block like the eRPM block 250 would do it).
  Neither is done here: the issue asks for a model and a mapping, not a SITL patch, and inventing an input SITL does not
  have is out of scope. It is the natural follow-up, written down in "Follow-ups".
- "The mapping Betaflight needs" is therefore already in place: `attitude_flu_nwu` (FRD→NED to FLU→NWU, verified in M0) is
  exactly the rotation SITL's virtual compass applies. This change adds a test that pins the simulator's **default
  world field** to SITL's constants, so that the simulated compass and Betaflight's virtual one describe the same
  field (comparing `mag.heading_rad` with Betaflight's heading shows the same declination).

## Goals

1. `ofs_sensors::mag::Mag`, a model next to `Imu` and `Baro`: it reads the true attitude and the world's earth field and
   publishes `mag.field_frd_ut` (µT, body FRD) and `mag.heading_rad` (the magnetic heading a compass would compute).
2. Measured field = `soft_iron · (Rᵀ · B_earth) + hard_iron + noise`, the standard calibration model (the one a
   magnetometer calibration fits: an ellipsoid offset by the hard iron).
3. Pure functions, unit-tested first: `earth_field_ned(total, inclination, declination)`, `magnetic_heading(field_frd,
   attitude)` (tilt-compensated), `true_heading(magnetic, declination)`.
4. The world carries the field (`[environment]`: total intensity, inclination, declination, all defaulted), the way it
   already carries `ambient_c`.
5. The magnetometer is a **library part** (`library/magnetometers/<id>.toml`, `[mag]`, per-parameter provenance) that a
   quad references with `mag = "<id>"`, resolved like the IMU and the barometer. One shipped part, `opendrone-mag`.
6. Existing flights do not change: `mag` is optional, a quad without it has no compass, registers no signal, draws no
   random number. Every golden, digest and recording stays as is.

## Non-goals

- **Any SITL input** (see above), and with it any Godot, telemetry or protocol change.
- **A datasheet-sourced part.** The ST LIS3MDL datasheet could not be read in this environment (the PDF came back as
  compressed streams), and noise figures quoted second hand in forums are not a source. `opendrone-mag` is `estimated`; a
  datasheet magnetometer is a data-only follow-up, as the datasheet barometer was for #42.
- **Magnetic anomalies** (steel structures, a local field), **temperature drift**, **motor/battery current-induced
  interference** (a real quad's dominant error; hard iron approximates it statically) and **saturation / range**.
  Interference could later be a `fault.mag.*` seam; none is added now.
- **A field model** (WMM/IGRF) or latitude-derived defaults: the world states its field; the default is a round number
  that matches SITL, not a lookup for the home position.
- **An estimated vertical reference**: `mag.heading_rad` uses the true roll and pitch (what an accelerometer-aided
  attitude converges to).

## Rulings

**R1. Part file.** `library/magnetometers/<id>.toml`: `[mag]` with `noise_std_ut` (per output sample, µT), `rate_hz`
(output data rate, Hz), `hard_iron_ut` (`[x, y, z]`, body FRD, µT), `soft_iron` (3×3 row-major, identity = none), plus
`[provenance."mag.<field>"]` for each. `PART_SCHEMA_VERSION` stays 1. Hard and soft iron are in the part like the IMU's
biases are, as data; the shipped part has zero offset and an identity matrix (those belong to one installation, and the
note says so).

**R2. Rate.** The model runs every `base_hz / rate_hz` ticks (`rate_hz` must divide `sim.base_hz`, like
`fc.exchange_hz`), so `noise_std_ut` is a per-output-sample figure, as a datasheet gives it, and needs no bandwidth
conversion; the signals hold between samples. Shipped part: 100 Hz.

**R3. Quad syntax and schema.** `mag = "opendrone-mag"` beside `imu =`/`baro =`, optional. **Schema 6 → 7**; `mag`
needs schema 7. `MIN_SCHEMA_VERSION` stays 5; schema-5 and 6 files load unchanged. `PartRefs.mag: Option<String>`,
`QuadConfig.mag: Option<MagSection>`. Provenance order `…, imu, baro, mag, vtx`. No shipped quad references it: a
flight controller with no compass is the common build, and "a new sensor does not change existing flights" is safest
kept literal. The README shows the one line that adds it.

**R4. World field.** `[environment]` gains `magnetic_field_ut` (default 50.0), `magnetic_inclination_deg` (default 60.0,
positive = field points down) and `magnetic_declination_deg` (default 2.0, positive = east). The defaults reproduce
SITL's direction (60°, 2° E); 50 µT is the author's round typical mid-latitude magnitude (the earth's field spans
roughly 25–65 µT), documented as a round number and not a lookup. World schema stays 1 (additive defaults, like
`ambient_c`). Validated: field in (0, 100] µT, inclination in [-90, 90], declination in [-180, 180].

**R5. Heading.** Tilt-compensated, FRD: with roll φ and pitch θ taken from the true attitude,
`Xh = Bx·cosθ + By·sinφ·sinθ + Bz·cosφ·sinθ`, `Yh = By·cosφ − Bz·sinφ`, magnetic heading `= atan2(−Yh, Xh)` in
[0, 2π). A level quad at yaw ψ in a field with declination D reads `ψ − D` (the angle to magnetic north);
`true_heading(m, D) = m + D`. Hard iron and noise shift the reading, so a miscalibration shows as a heading error.

**R6. Signals.** `mag.field_frd_ut` (Vec3), `mag.heading_rad` (f64), registered only when the quad has a `mag`. They take
the recorder's default rate like the other sensor outputs.

## Tests

`ofs-sensors`: earth field components for known inclination/declination; level heading equals −declination at yaw 0 and
tracks yaw; heading is invariant under roll/pitch (tilt compensation) at several attitudes; a hard-iron offset shifts the
field exactly; soft-iron scaling applies before the offset; noise has the configured std and is seeded; the model runs
only at its rate (the signal holds between samples); the default field matches SITL's constants (direction). `ofs-config`:
part loader (good file, missing provenance), shipped part loads, a quad with `mag` at schema 7 loads and lists the part,
`mag` at schema 6 is refused naming the field, schema-5/6 quads unchanged, an unknown id lists what `magnetometers/`
holds, `rate_hz` not dividing `base_hz` refused, world field validation. `ofs-sim`: a quad with a mag registers the
signals and a run reads a heading that tracks the initial yaw; shipped quads register none.

## Follow-ups (not done here)

- Feed SITL a measured field: extend the state datagram with a reserved block (like eRPM block 250) and patch
  `virtualMagSet` to take it, so noise, hard iron and the world's field reach Betaflight's heading hold / GPS rescue.
- A datasheet magnetometer part; a `fault.mag.*` interference seam.
