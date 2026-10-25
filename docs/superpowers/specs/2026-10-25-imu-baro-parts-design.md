# The IMU and the barometer as library parts (#42)

Closes #42. A data and config change: `ofs-sensors`, the simulation, the protocol and every golden stay as they are.

## Why

Motors, props, ESCs, batteries and VTXs are component-library parts with per-parameter provenance (M4a). The IMU and the
barometer are still two inline tables in every quad file (`[imu]`, `[baro]`), with no provenance and no way to say "this
quad carries an ICM-42688-P". The issue's brief is its three lines; no design conversation took place, so every ruling
below is the author's call and is open to change on review.

## Goals

1. A quad references an IMU and a barometer part by id, resolved like every other part (`library/imus/<id>.toml`,
   `library/barometers/<id>.toml`, the `OFS_LIBRARY` search path, the same "not found" message listing what a
   directory holds).
2. The five shipped quads reference `opendrone-imu` and `opendrone-baro`, whose values are the numbers the quads carried
   (`0.002 rad/s`, `0.05 m/s²`, zero biases, `2.0 Pa`): the simulation is unchanged, so no golden, digest or recording moves.
3. Every parameter of the new parts carries provenance, a part with a gap is refused at load (the existing check), and
   the parts appear in `LoadReply.parts` / `sim.parts` with their provenance like the other kinds.
4. One datasheet-sourced IMU, `tdk-icm-42688p`, so the library has a part whose numbers come from a datasheet, with
   the arithmetic from datasheet figure to model parameter written in its provenance. The research page lists the sources.

## Non-goals

- **Datasheet ranges** (gyro full scale, accelerometer range): the model has no saturation, so a range would be a
  parameter nothing reads. A part may grow one the day the model clips.
- **A real barometer part.** The datasheet figures that would feed `noise_std_pa` depend on the oversampling mode and the
  bandwidth, and the copy of the BMP388 datasheet reachable when this was written could not be read; the shipped
  `opendrone-baro` stays `estimated`. A datasheet barometer is a data-only follow-up.
- Per-unit bias in a part: a bias is a property of one unit, not of the part number. Parts ship `[0, 0, 0]` (the
  datasheet gives only a tolerance); the `fault.imu.*` seams remain the way to bias a run.
- Overrides of a part's values in the quad file.
- Moving the quad's `[imu]`/`[baro]` wiring anywhere: they have none (the model runs at the base rate on the bus).

## Rulings

**R1. Quad syntax.** `imu = "opendrone-imu"` and `baro = "opendrone-baro"`, top-level strings beside `frame = …`
(neither has wiring, unlike `[battery]` and `[vtx]`). Quad schema **5 → 6**.

**R2. Schema-5 inline tables keep loading.** `MIN_SCHEMA_VERSION` stays 5: a schema-5 file with `[imu]`/`[baro]` tables
loads exactly as before (no provenance; `parts.imu` is `None`). A string at schema 5, or a table at schema 6, is a
validation problem naming the field. This keeps older repro bundles and users' own quads working, at the cost of two
spellings in the quad reader (an untagged serde enum) that a later schema can drop.

**R3. Part files.** `[imu]`: `gyro_noise_std_radps`, `gyro_bias_radps`, `accel_noise_std_mps2`, `accel_bias_mps2`;
`[baro]`: `noise_std_pa`. `ImuSection` and `BaroSection` stay the config types the simulator reads; the part structs
wrap them as `MotorPart` wraps `MotorSection`. `PART_SCHEMA_VERSION` stays 1 (a new kind is a new file shape, not a
change to an old one).

**R4. Provenance order and kinds.** `part_provenance` lists `…, battery, imu, baro, vtx`; the kind strings are `imu` and
`baro`; the library directories are `imus/` and `barometers/` (the issue's names).

**R5. Noise arithmetic for `tdk-icm-42688p`.** The model draws white noise once per base-rate sample (8 kHz), so the
per-sample deviation is the noise density times the square root of the noise bandwidth, taken as the Nyquist bandwidth
`base_hz / 2 = 4000 Hz`: gyro `2.8 mdps/√Hz · 63.25 √Hz = 0.177 dps = 3.09e-3 rad/s`, accelerometer
`70 µg/√Hz · 9.80665 · 63.25 √Hz = 0.0434 m/s²`. Datasheet figures: TDK DS-000347 v1.9 (low-noise mode). The part's
note says the bandwidth assumption is the simulator's, not the chip's, and `source = "datasheet"` applies to the density,
while the conversion is arithmetic on it.

## Tests

`ofs-config`: part loaders (a good file, missing provenance, unknown provenance path); every shipped part loads; a quad
referencing the shipped IMU and baro loads with the old numbers and lists both parts in `part_provenance`; schema
mismatches (string at 5, table at 6) are refused with the field named; an unknown id lists what `imus/` holds; a schema-5
inline quad still loads with `parts.imu == None`. `ofs-sim` gRPC and Python `sim.parts` kind lists gain `imu` and `baro`.
The existing goldens and digests are the proof the simulation is unchanged.
