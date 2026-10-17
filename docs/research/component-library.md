# The component library's provenance

Every value shipped in `library/` is an M1 estimate, recorded in the open. Since M4a a quad file (schema 5)
references parts instead of carrying hardware — one TOML per part under `library/` (see the "Component
library" section of README.md) — and every parameter of a part file must carry a `[provenance]` entry: a
`source` (`datasheet`, `measured` or `estimated`), an optional `link` and an optional `note`. The loader
refuses a part file whose data skips an entry, and one whose provenance names a parameter that does not
exist; a table-valued parameter is one entry (`prop.ct_table` is one parameter, however long its rows).
This note is the ledger: one table per shipped part file — seven files, six kinds, 46 parameters —
transcribed as shipped at M4a.

The follow-up is datasheet transcription: the OpenMotor motor and the OpenVTX video transmitters are open
hardware with published specifications, and anyone can fill the real numbers in. It lands by editing the
part file — the parameter's value and its provenance entry (`source = "datasheet"`, the link, a note) — and
nothing else. No code, no quad file, no other part changes; a quad picks the new value up on its next load.

Two values deserve their own sentence. `motor.pole_pairs = 7`: the OpenMotor 2207 is a 14-pole motor, and
2 × 7 is Betaflight's `motor_poles` default of 14, so a quad whose Betaflight diff leaves `motor_poles`
unset agrees by construction (a diff that sets it must match — SITL quads are checked at load).
`prop.imbalance_kgm = 0.0`: there is no measurement, and the parameter is the M4b vibration model's input —
it stays 0 until that model lands.

## `frames/opendrone-5f-softmount.toml` — OpenDrone 5F frame, soft-mounted FC

The same frame as below plus a flight-controller mount (`[frame.fc_mount]`), the example of a soft-mounted stack. A
frame part with no `[frame.fc_mount]` is rigid. It is a library part only: no shipped quad uses it, so a quad opts in
with `frame = "opendrone-5f-softmount"`.

| Parameter | Value | Source | Note |
|---|---|---|---|
| `frame.fc_mount.natural_hz` | 120.0 | estimated | rubber-grommet FC stack, order of magnitude; not measured |
| `frame.fc_mount.damping_ratio` | 0.15 | estimated | rubber-grommet FC stack, order of magnitude; not measured |

Every other parameter equals `opendrone-5f`'s. The mount's natural frequency must be at most `sim.base_hz / 10`.

## `frames/opendrone-5f.toml` — OpenDrone 5F frame

| Parameter | Value | Source | Note |
|---|---|---|---|
| `frame.mass_kg` | 0.65 | estimated | M1 estimate |
| `frame.inertia_kgm2` | [0.0025, 0.0025, 0.0045] | estimated | M1 estimate |
| `frame.cda_m2` | [0.010, 0.010, 0.020] | estimated | M1 estimate |
| `frame.angular_damping_nms` | 0.0001 | estimated | M1 estimate |
| `frame.motor_positions_frd_m` | [-0.08, 0.08, 0], [0.08, 0.08, 0], [-0.08, -0.08, 0], [0.08, -0.08, 0] | estimated | M1 estimate, a 5" X-frame's arm length |
| `frame.motor_spin` | [1, -1, -1, 1] | datasheet | Betaflight Quad-X order (https://betaflight.com/docs/development/Mixer) |
| `frame.contact_points_frd_m` | the four motor positions at z = 0.03 | estimated | M1 estimate, legs under the arms |
| `ground.stiffness_npm` | 3000.0 | estimated | M1 estimate |
| `ground.damping_nspm` | 40.0 | estimated | M1 estimate |
| `ground.friction_coeff` | 0.6 | estimated | M1 estimate |

## `motors/opendrone-openmotor-2207.toml` — OpenMotor 2207 1950KV

| Parameter | Value | Source | Note |
|---|---|---|---|
| `motor.kv_rpm_per_v` | 1950.0 | estimated | M1 estimate; the OpenMotor datasheet value replaces this when transcribed |
| `motor.resistance_ohm` | 0.07 | estimated | M1 estimate |
| `motor.no_load_current_a` | 1.2 | estimated | M1 estimate |
| `motor.rotor_inertia_kgm2` | 6.0e-6 | estimated | M1 estimate |
| `motor.pole_pairs` | 7 | estimated | 14-pole motor; matches Betaflight's `motor_poles` default of 14 |
| `motor.thermal.heat_capacity_jpk` | 15.0 | estimated | lumped RC estimate (about 30 g of copper and steel); not measured |
| `motor.thermal.resistance_kpw` | 1.5 | estimated | lumped RC estimate, with prop wash over the bell; not measured |

## `props/opendrone-5in-prop.toml` — OpenDrone 5in prop

| Parameter | Value | Source | Note |
|---|---|---|---|
| `prop.diameter_m` | 0.127 | estimated | 5 inch |
| `prop.inertia_kgm2` | 6.0e-6 | estimated | M1 estimate |
| `prop.ct_table` | C_T = 0.11, flat over 0–30000 rad/s | estimated | M1 flat-table estimate |
| `prop.cp_table` | C_P = 0.045, flat over 0–30000 rad/s | estimated | M1 flat-table estimate |
| `prop.imbalance_kgm` | 0.0 | estimated | no measurement; 0 until the M4b vibration model lands |
| `prop.windmill_j0` | 1.0 | estimated | zero-torque advance ratio; order-of-magnitude estimate, not measured; 0 turns windmilling off, otherwise at least 0.2 |

## `escs/opendrone-4in1-esc.toml` — OpenDrone 4-in-1 ESC (AM32)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `esc.response_tau_s` | 0.002 | estimated | M1 estimate |
| `esc.current_limit_a` | 45.0 | estimated | M1 estimate |
| `esc.thermal.heat_capacity_jpk` | 3.0 | estimated | lumped RC estimate (a few grams of board and FETs); not measured |
| `esc.thermal.resistance_kpw` | 8.0 | estimated | lumped RC estimate; not measured |
| `esc.thermal.r_on_ohm` | 0.004 | estimated | a pair of FETs per phase, order of magnitude; not measured |
| `esc.thermal.switching_loss_coeff` | 0.0024 | estimated | 0.5 · (rise + fall time 100 ns) · 48 kHz, per volt and amp; not measured |

## `batteries/opendrone-6s-1300.toml` — OpenDrone 6S 1300 mAh

| Parameter | Value | Source | Note |
|---|---|---|---|
| `battery.cells` | 6 | estimated | 6S |
| `battery.capacity_mah` | 1300.0 | estimated | M1 estimate |
| `battery.r0_ohm` | 0.024 | estimated | M1 estimate |
| `battery.r1_ohm` | 0.010 | estimated | M1 estimate |
| `battery.c1_f` | 2000.0 | estimated | M1 estimate |
| `battery.ocv_table` | 3.27 V (0 %) rising to 4.20 V (100 %), 12 points | estimated | M1 estimate, a generic LiPo curve |

A battery part may also set `cell_r0_scale` and `cell_capacity_scale` (empty, or one positive multiplier per cell) and
the ageing keys `cycles`, `capacity_fade_per_cycle`, `resistance_growth_per_cycle` (default 0); the shipped pack sets
none of them. See `docs/research/battery-and-thermal.md`.

## `batteries/opendrone-6s-1300-aged.toml` — OpenDrone 6S 1300 mAh, aged

The same pack as above, worn: 150 cycles and one tired cell. An example of the spread and ageing keys; no shipped quad
uses it (a quad opts in with `part = "opendrone-6s-1300-aged"` in its `[battery]`).

| Parameter | Value | Source | Note |
|---|---|---|---|
| `battery.cell_r0_scale` | [1.0, 1.0, 1.0, 1.2, 1.0, 1.0] | estimated | a worn pack with one tired cell; not measured |
| `battery.cycles` | 150 | estimated | a worn pack; not measured |
| `battery.capacity_fade_per_cycle` | 0.001 | estimated | 0.1 % of the capacity per cycle, an order of magnitude; not measured |
| `battery.resistance_growth_per_cycle` | 0.004 | estimated | 0.4 % of R0 per cycle, an order of magnitude; not measured |

## `vtx/opendrone-smartaudio-1w.toml` — OpenDrone SmartAudio VTX (1 W)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `vtx.kind` | "smartaudio" | estimated | SmartAudio v2.1 behavioural model (M3a) |
| `vtx.power_levels_mw` | [25, 200, 600, 1000] | estimated | M1 estimate |
| `vtx.power_levels_dbm` | [14, 23, 28, 30] | estimated | dBm equivalents of the mW table, our own conversion |
| `vtx.reply_latency_ms` | 5.0 | estimated | M3a estimate |
| `vtx.pit_power_mw` | 0.1 | estimated | M3a estimate: a trickle next to the pilot |
| `vtx.antenna.kind` | "omni" | estimated | M3b estimate |
| `vtx.antenna.gain_dbi` | 2.0 | estimated | M3b estimate |
| `vtx.antenna.polarization` | "rhcp" | estimated | M3b estimate |
| `vtx.antenna.mount_frd` | [-0.5, 0.0, -1.0] | estimated | M3b estimate |
| `vtx.thermal.heat_capacity_jpk` | 2.0 | estimated | lumped RC estimate (a few grams of board and shield); not measured |
| `vtx.thermal.resistance_kpw` | 15.0 | estimated | lumped RC estimate, little airflow; not measured |
| `vtx.thermal.efficiency` | 0.25 | estimated | RF output over DC input, order of magnitude; not measured |
| `vtx.thermal.idle_w` | 0.4 | estimated | draw with no RF output, order of magnitude; not measured |

## `vtx/opendrone-tramp-600.toml` — OpenDrone Tramp VTX (600 mW)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `vtx.kind` | "tramp" | estimated | Tramp behavioural model (M3e) |
| `vtx.power_levels_mw` | [25, 100, 200, 400, 600] | datasheet | the FC driver's built-in trampPowerTable (https://github.com/betaflight/betaflight/blob/master/src/main/io/vtx_tramp.c) |
| `vtx.power_levels_dbm` | [14, 20, 23, 26, 28] | estimated | labels only; the codec never sends dBm |
| `vtx.reply_latency_ms` | 5.0 | estimated | M3e estimate |
| `vtx.pit_power_mw` | 0.1 | estimated | default 0.1 (M3a) |
| `vtx.antenna.kind` | "omni" | estimated | same antenna as the SmartAudio quad |
| `vtx.antenna.gain_dbi` | 2.0 | estimated | M3b estimate |
| `vtx.antenna.polarization` | "rhcp" | estimated | M3b estimate |
| `vtx.antenna.mount_frd` | [-0.5, 0.0, -1.0] | estimated | M3b estimate |
| `vtx.thermal.heat_capacity_jpk` | 2.0 | estimated | lumped RC estimate (a few grams of board and shield); not measured |
| `vtx.thermal.resistance_kpw` | 15.0 | estimated | lumped RC estimate, little airflow; not measured |
| `vtx.thermal.efficiency` | 0.25 | estimated | RF output over DC input, order of magnitude; not measured |
| `vtx.thermal.idle_w` | 0.4 | estimated | draw with no RF output, order of magnitude; not measured |
