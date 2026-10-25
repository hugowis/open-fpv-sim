# The component library's provenance

Every value shipped in `library/` is an estimate, recorded in the open (M1's for the 5" parts, class-typical ones for the
parts of the toothpick, race and long-range quads), except the datasheet noise densities of the ICM-42688-P, the
Quad-X motor order and the Tramp power table (both Betaflight's). Since M4a a quad file references parts instead of carrying hardware (the IMU and the barometer
joined in schema 6) — one TOML per part under `library/` (see the "Component
library" section of README.md) — and every parameter of a part file must carry a `[provenance]` entry: a
`source` (`datasheet`, `measured` or `estimated`), an optional `link` and an optional `note`. The loader
refuses a part file whose data skips an entry, and one whose provenance names a parameter that does not
exist; a table-valued parameter is one entry (`prop.ct_table` is one parameter, however long its rows).
This note is the ledger: one table per part file — 26 files, eight kinds, 198 parameters. The first seven files are as
shipped at M4a; the aged pack, the soft-mount frame and the IMU and barometer parts came later; the last fourteen
are the parts of the toothpick, race and long-range quads (#129).

The follow-up is datasheet transcription: the OpenMotor motor and the OpenVTX video transmitters are open
hardware with published specifications, and anyone can fill the real numbers in. It lands by editing the
part file — the parameter's value and its provenance entry (`source = "datasheet"`, the link, a note) — and
nothing else. No code, no quad file, no other part changes; a quad picks the new value up on its next load.

The parts of #129 (`opendrone-3t`, `opendrone-5r`, `opendrone-7lr` and their motors, props, ESCs, packs and the 400 mW
VTX) are all `estimated`, except the Quad-X motor order. No figure in them was read from a datasheet: each is a
class-typical value for its size, chosen with a steady-state model of the simulator's own equations so the quad lands
where its class does (thrust-to-weight, hover throttle, hover power), and each note says what the value was scaled from.
Where a note says "opendrone-5f's value scaled by …", the 5F parts are the reference.

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

## `frames/opendrone-3t.toml` — OpenDrone 3T frame

| Parameter | Value | Source | Note |
|---|---|---|---|
| `frame.mass_kg` | 0.105 | estimated | all-up weight of a 3S 450 mAh toothpick (frame 18 g, four 1404 motors 24 g, AIO board 12 g, camera and VTX 8 g, props 4 g, pack 39 g); a sum of class-typical masses, not weighed |
| `frame.inertia_kgm2` | [0.00012, 0.00012, 0.00022] | estimated | 0.3 * mass * (arm radius)^2 about x and y, and 1.8 times that about z: the opendrone-5f relation scaled to this size; not measured |
| `frame.cda_m2` | [0.003, 0.003, 0.006] | estimated | opendrone-5f's drag areas scaled by the square of the size; not measured |
| `frame.angular_damping_nms` | 5e-06 | estimated | opendrone-5f's value scaled by the inertia; not measured |
| `frame.motor_positions_frd_m` | [-0.044, 0.044, 0.0], [0.044, 0.044, 0.0], [-0.044, -0.044, 0.0], [0.044, -0.044, 0.0] | estimated | 125 mm wheelbase: 0.044 m = 125 mm / 2 / sqrt(2); not measured |
| `frame.motor_spin` | [1, -1, -1, 1] | datasheet | Betaflight Quad-X order (https://betaflight.com/docs/development/Mixer) |
| `frame.contact_points_frd_m` | the four motor positions at z = 0.02 | estimated | legs under the arms, 2 cm below the arms |
| `ground.stiffness_npm` | 500.0 | estimated | opendrone-5f's 3000 N/m scaled by the mass; not measured |
| `ground.damping_nspm` | 6.5 | estimated | chosen with the stiffness so the contact's damping ratio is opendrone-5f's 0.45; not measured |
| `ground.friction_coeff` | 0.6 | estimated | opendrone-5f's value |

## `frames/opendrone-5r.toml` — OpenDrone 5R frame

| Parameter | Value | Source | Note |
|---|---|---|---|
| `frame.mass_kg` | 0.54 | estimated | all-up weight of a 6S 1300 mAh race quad (frame 105 g, four 2306 motors 128 g, stack 45 g, camera and VTX 25 g, props 16 g, pack 180 g, wiring and straps 41 g); a sum of class-typical masses, not weighed |
| `frame.inertia_kgm2` | [0.002, 0.002, 0.0036] | estimated | 0.3 * mass * (arm radius)^2 about x and y, and 1.8 times that about z: the opendrone-5f relation scaled to this size; not measured |
| `frame.cda_m2` | [0.008, 0.008, 0.018] | estimated | opendrone-5f's drag areas scaled by the square of the size; not measured |
| `frame.angular_damping_nms` | 6e-05 | estimated | opendrone-5f's value scaled by the inertia; not measured |
| `frame.motor_positions_frd_m` | [-0.078, 0.078, 0.0], [0.078, 0.078, 0.0], [-0.078, -0.078, 0.0], [0.078, -0.078, 0.0] | estimated | 220 mm wheelbase: 0.078 m = 220 mm / 2 / sqrt(2); not measured |
| `frame.motor_spin` | [1, -1, -1, 1] | datasheet | Betaflight Quad-X order (https://betaflight.com/docs/development/Mixer) |
| `frame.contact_points_frd_m` | the four motor positions at z = 0.03 | estimated | legs under the arms, 3 cm below the arms |
| `ground.stiffness_npm` | 2500.0 | estimated | opendrone-5f's 3000 N/m scaled by the mass; not measured |
| `ground.damping_nspm` | 33.0 | estimated | chosen with the stiffness so the contact's damping ratio is opendrone-5f's 0.45; not measured |
| `ground.friction_coeff` | 0.6 | estimated | opendrone-5f's value |

## `frames/opendrone-7lr.toml` — OpenDrone 7LR frame

| Parameter | Value | Source | Note |
|---|---|---|---|
| `frame.mass_kg` | 0.9 | estimated | all-up weight of a 6S 4000 mAh Li-ion long-range quad (frame 140 g, four 2806.5 motors 168 g, stack 50 g, camera, VTX, GPS and receiver 60 g, props 36 g, pack 410 g, wiring and straps 36 g); a sum of class-typical masses, not weighed |
| `frame.inertia_kgm2` | [0.0069, 0.0069, 0.0124] | estimated | 0.3 * mass * (arm radius)^2 about x and y, and 1.8 times that about z: the opendrone-5f relation scaled to this size; not measured |
| `frame.cda_m2` | [0.016, 0.016, 0.035] | estimated | opendrone-5f's drag areas scaled by the square of the size; not measured |
| `frame.angular_damping_nms` | 0.00028 | estimated | opendrone-5f's value scaled by the inertia; not measured |
| `frame.motor_positions_frd_m` | [-0.113, 0.113, 0.0], [0.113, 0.113, 0.0], [-0.113, -0.113, 0.0], [0.113, -0.113, 0.0] | estimated | 320 mm wheelbase: 0.113 m = 320 mm / 2 / sqrt(2); not measured |
| `frame.motor_spin` | [1, -1, -1, 1] | datasheet | Betaflight Quad-X order (https://betaflight.com/docs/development/Mixer) |
| `frame.contact_points_frd_m` | the four motor positions at z = 0.04 | estimated | legs under the arms, 4 cm below the arms |
| `ground.stiffness_npm` | 4150.0 | estimated | opendrone-5f's 3000 N/m scaled by the mass; not measured |
| `ground.damping_nspm` | 55.0 | estimated | chosen with the stiffness so the contact's damping ratio is opendrone-5f's 0.45; not measured |
| `ground.friction_coeff` | 0.6 | estimated | opendrone-5f's value |

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

## `motors/opendrone-motor-1404.toml` — OpenDrone 1404 3000KV

| Parameter | Value | Source | Note |
|---|---|---|---|
| `motor.kv_rpm_per_v` | 3000.0 | estimated | a 1404 motor for a 3-inch 3S quad; a class-typical value, not a datasheet figure |
| `motor.resistance_ohm` | 0.14 | estimated | a 1404 motor for a 3-inch 3S quad; a class-typical phase resistance, not measured |
| `motor.no_load_current_a` | 0.6 | estimated | a 1404 motor for a 3-inch 3S quad; a class-typical value, not measured |
| `motor.rotor_inertia_kgm2` | 1.2e-06 | estimated | scaled from opendrone-openmotor-2207's by the bell's size; not measured |
| `motor.pole_pairs` | 7 | estimated | 14-pole motor (12N14P); matches Betaflight's motor_poles default of 14 |
| `motor.thermal.heat_capacity_jpk` | 5.0 | estimated | lumped RC estimate from the motor's copper and steel mass; not measured |
| `motor.thermal.resistance_kpw` | 3.0 | estimated | lumped RC estimate, with prop wash over the bell; not measured |

## `motors/opendrone-motor-2306.toml` — OpenDrone 2306 2450KV

| Parameter | Value | Source | Note |
|---|---|---|---|
| `motor.kv_rpm_per_v` | 2450.0 | estimated | a 2306 motor for a 5-inch 6S race quad; a class-typical value, not a datasheet figure |
| `motor.resistance_ohm` | 0.06 | estimated | a 2306 motor for a 5-inch 6S race quad; a class-typical phase resistance, not measured |
| `motor.no_load_current_a` | 1.5 | estimated | a 2306 motor for a 5-inch 6S race quad; a class-typical value, not measured |
| `motor.rotor_inertia_kgm2` | 6.5e-06 | estimated | scaled from opendrone-openmotor-2207's by the bell's size; not measured |
| `motor.pole_pairs` | 7 | estimated | 14-pole motor (12N14P); matches Betaflight's motor_poles default of 14 |
| `motor.thermal.heat_capacity_jpk` | 17.0 | estimated | lumped RC estimate from the motor's copper and steel mass; not measured |
| `motor.thermal.resistance_kpw` | 1.4 | estimated | lumped RC estimate, with prop wash over the bell; not measured |

## `motors/opendrone-motor-2806.toml` — OpenDrone 2806.5 1300KV

| Parameter | Value | Source | Note |
|---|---|---|---|
| `motor.kv_rpm_per_v` | 1300.0 | estimated | a 2806.5 motor for a 7-inch 6S long-range quad; a class-typical value, not a datasheet figure |
| `motor.resistance_ohm` | 0.085 | estimated | a 2806.5 motor for a 7-inch 6S long-range quad; a class-typical phase resistance, not measured |
| `motor.no_load_current_a` | 0.8 | estimated | a 2806.5 motor for a 7-inch 6S long-range quad; a class-typical value, not measured |
| `motor.rotor_inertia_kgm2` | 1.1e-05 | estimated | scaled from opendrone-openmotor-2207's by the bell's size; not measured |
| `motor.pole_pairs` | 7 | estimated | 14-pole motor (12N14P); matches Betaflight's motor_poles default of 14 |
| `motor.thermal.heat_capacity_jpk` | 25.0 | estimated | lumped RC estimate from the motor's copper and steel mass; not measured |
| `motor.thermal.resistance_kpw` | 1.2 | estimated | lumped RC estimate, with prop wash over the bell; not measured |

## `props/opendrone-5in-prop.toml` — OpenDrone 5in prop

| Parameter | Value | Source | Note |
|---|---|---|---|
| `prop.diameter_m` | 0.127 | estimated | 5 inch |
| `prop.inertia_kgm2` | 6.0e-6 | estimated | M1 estimate |
| `prop.ct_table` | C_T = 0.11, flat over 0–30000 rad/s | estimated | M1 flat-table estimate |
| `prop.cp_table` | C_P = 0.045, flat over 0–30000 rad/s | estimated | M1 flat-table estimate |
| `prop.imbalance_kgm` | 0.0 | estimated | no measurement; 0 until the M4b vibration model lands |
| `prop.windmill_j0` | 1.0 | estimated | zero-torque advance ratio; order-of-magnitude estimate, not measured; 0 turns windmilling off, otherwise at least 0.2 |

## `props/opendrone-3in-prop.toml` — OpenDrone 3in prop

| Parameter | Value | Source | Note |
|---|---|---|---|
| `prop.diameter_m` | 0.076 | estimated | 3 inch (76 mm) |
| `prop.inertia_kgm2` | 7e-07 | estimated | a 1.5 g two-blade prop as a rod of its length, 1/12 * m * L^2; not measured |
| `prop.ct_table` | C_T = 0.09, flat over 0–60000 rpm | estimated | flat-table estimate, a little under opendrone-5in-prop's 0.11 for the lower Reynolds number of a small blade; not measured |
| `prop.cp_table` | C_P = 0.05, flat over 0–60000 rpm | estimated | flat-table estimate, a little above opendrone-5in-prop's 0.045 (small blades are less efficient); not measured |
| `prop.imbalance_kgm` | 0.0 | estimated | no measurement; 0 until a vibration measurement exists |
| `prop.windmill_j0` | 1.0 | estimated | zero-torque advance ratio of a small propeller; order-of-magnitude estimate, not measured |

## `props/opendrone-5in-race-prop.toml` — OpenDrone 5in race prop

| Parameter | Value | Source | Note |
|---|---|---|---|
| `prop.diameter_m` | 0.127 | estimated | 5 inch |
| `prop.inertia_kgm2` | 5.5e-06 | estimated | a 4.5 g two-blade race prop, 1/12 * m * L^2 with a hub allowance; not measured |
| `prop.ct_table` | C_T = 0.125, flat over 0–60000 rpm | estimated | flat-table estimate: a steeper pitch than opendrone-5in-prop's 0.11 gives more thrust per rev^2; not measured |
| `prop.cp_table` | C_P = 0.055, flat over 0–60000 rpm | estimated | flat-table estimate: the steeper pitch also takes more power per rev^3 than opendrone-5in-prop's 0.045; not measured |
| `prop.imbalance_kgm` | 0.0 | estimated | no measurement; 0 until a vibration measurement exists |
| `prop.windmill_j0` | 1.0 | estimated | zero-torque advance ratio of a small propeller; order-of-magnitude estimate, not measured |

## `props/opendrone-7in-prop.toml` — OpenDrone 7in prop

| Parameter | Value | Source | Note |
|---|---|---|---|
| `prop.diameter_m` | 0.178 | estimated | 7 inch (177.8 mm) |
| `prop.inertia_kgm2` | 2e-05 | estimated | a 9 g two-blade prop as a rod of its length, 1/12 * m * L^2; not measured |
| `prop.ct_table` | C_T = 0.12, flat over 0–60000 rpm | estimated | flat-table estimate: a large, low-pitch blade turns well at low rpm; slightly above opendrone-5in-prop's 0.11; not measured |
| `prop.cp_table` | C_P = 0.05, flat over 0–60000 rpm | estimated | flat-table estimate: a little above opendrone-5in-prop's 0.045 for the longer blade; not measured |
| `prop.imbalance_kgm` | 0.0 | estimated | no measurement; 0 until a vibration measurement exists |
| `prop.windmill_j0` | 1.0 | estimated | zero-torque advance ratio of a small propeller; order-of-magnitude estimate, not measured |

## `escs/opendrone-4in1-esc.toml` — OpenDrone 4-in-1 ESC (AM32)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `esc.response_tau_s` | 0.002 | estimated | M1 estimate |
| `esc.current_limit_a` | 45.0 | estimated | M1 estimate |
| `esc.thermal.heat_capacity_jpk` | 3.0 | estimated | lumped RC estimate (a few grams of board and FETs); not measured |
| `esc.thermal.resistance_kpw` | 8.0 | estimated | lumped RC estimate; not measured |
| `esc.thermal.r_on_ohm` | 0.004 | estimated | a pair of FETs per phase, order of magnitude; not measured |
| `esc.thermal.switching_loss_coeff` | 0.0024 | estimated | 0.5 · (rise + fall time 100 ns) · 48 kHz, per volt and amp; not measured |

## `escs/opendrone-4in1-esc-20a.toml` — OpenDrone 4-in-1 ESC (20 A)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `esc.response_tau_s` | 0.002 | estimated | opendrone-4in1-esc's value |
| `esc.current_limit_a` | 20.0 | estimated | a 20 A-per-motor AIO board, class-typical |
| `esc.thermal.heat_capacity_jpk` | 2.0 | estimated | lumped RC estimate (a couple of grams of board and FETs); not measured |
| `esc.thermal.resistance_kpw` | 10.0 | estimated | lumped RC estimate; not measured |
| `esc.thermal.r_on_ohm` | 0.008 | estimated | a pair of FETs per phase, order of magnitude; not measured |
| `esc.thermal.switching_loss_coeff` | 0.0024 | estimated | 0.5 * (rise + fall time 100 ns) * 48 kHz switching, per volt and amp, as opendrone-4in1-esc; not measured |

## `escs/opendrone-4in1-esc-60a.toml` — OpenDrone 4-in-1 ESC (60 A)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `esc.response_tau_s` | 0.002 | estimated | opendrone-4in1-esc's value |
| `esc.current_limit_a` | 60.0 | estimated | a 60 A-per-motor race stack, class-typical; the 6S race quad draws about 50 A per motor at full throttle |
| `esc.thermal.heat_capacity_jpk` | 4.0 | estimated | lumped RC estimate (four grams of board and FETs); not measured |
| `esc.thermal.resistance_kpw` | 6.0 | estimated | lumped RC estimate; not measured |
| `esc.thermal.r_on_ohm` | 0.003 | estimated | a pair of FETs per phase, order of magnitude; not measured |
| `esc.thermal.switching_loss_coeff` | 0.0024 | estimated | 0.5 * (rise + fall time 100 ns) * 48 kHz switching, per volt and amp, as opendrone-4in1-esc; not measured |

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

## `batteries/opendrone-3s-450.toml` — OpenDrone 3S 450 mAh

| Parameter | Value | Source | Note |
|---|---|---|---|
| `battery.cells` | 3 | estimated | 3S |
| `battery.capacity_mah` | 450.0 | estimated | a common toothpick pack size |
| `battery.r0_ohm` | 0.045 | estimated | about 15 mohm per cell, a small high-rate pack; not measured |
| `battery.r1_ohm` | 0.018 | estimated | opendrone-6s-1300's value scaled up for the smaller pack; not measured |
| `battery.c1_f` | 1000.0 | estimated | opendrone-6s-1300's value scaled by the capacity; not measured |
| `battery.ocv_table` | 3.27 V (0 %) rising to 4.2 V (100 %), 12 points | estimated | a generic LiPo curve, as opendrone-6s-1300 (a Li-ion cell sits lower at the top and flatter; not modelled) |

## `batteries/opendrone-6s-4000.toml` — OpenDrone 6S 4000 mAh Li-ion

| Parameter | Value | Source | Note |
|---|---|---|---|
| `battery.cells` | 6 | estimated | 6S |
| `battery.capacity_mah` | 4000.0 | estimated | a 6S1P pack of 4000 mAh 21700 cells, the long-range quad's usual size |
| `battery.r0_ohm` | 0.06 | estimated | about 10 mohm per cell, a Li-ion cell that is built for energy and not for current; not measured |
| `battery.r1_ohm` | 0.025 | estimated | opendrone-6s-1300's value scaled up for the energy cell; not measured |
| `battery.c1_f` | 4000.0 | estimated | opendrone-6s-1300's value scaled by the capacity; not measured |
| `battery.ocv_table` | 3.27 V (0 %) rising to 4.2 V (100 %), 12 points | estimated | a generic LiPo curve, as opendrone-6s-1300 (a Li-ion cell sits lower at the top and flatter; not modelled) |

## `imus/opendrone-imu.toml` — OpenDrone IMU (generic 6-axis)

The numbers every shipped quad carried inline before the IMU became a part (#42), so a quad that references it flies
exactly as before. A bias belongs to one unit rather than to the part number, so both biases are zero; the `fault.imu.*`
seams bias a run.

| Parameter | Value | Source | Note |
|---|---|---|---|
| `imu.gyro_noise_std_radps` | 0.002 | estimated | M1 estimate of the white noise per base-rate sample (0.11 dps); not measured |
| `imu.gyro_bias_radps` | [0.0, 0.0, 0.0] | estimated | zero, see above |
| `imu.accel_noise_std_mps2` | 0.05 | estimated | M1 estimate of the white noise per base-rate sample; not measured |
| `imu.accel_bias_mps2` | [0.0, 0.0, 0.0] | estimated | zero, see above |

## `imus/tdk-icm-42688p.toml` — TDK InvenSense ICM-42688-P

The IMU of many current flight controllers, and the library's one datasheet-sourced sensor. The simulator draws white
noise once per base-rate sample (8 kHz), so a noise density becomes a per-sample deviation by the square root of the
Nyquist bandwidth, 4000 Hz (63.25 √Hz); that bandwidth is the simulator's assumption, not the chip's. Source: TDK
InvenSense product page (https://invensense.tdk.com/products/icm-42688-p/), datasheet DS-000347 v1.9, low-noise mode.
Full-scale ranges (gyro ±15.6 to ±2000 dps, accelerometer ±2 to ±16 g) are not parameters: the model does not clip.

| Parameter | Value | Source | Note |
|---|---|---|---|
| `imu.gyro_noise_std_radps` | 0.00309 | datasheet | 2.8 mdps/√Hz = 4.887e-5 rad/s/√Hz, times √4000 Hz |
| `imu.gyro_bias_radps` | [0.0, 0.0, 0.0] | estimated | the datasheet gives a zero-rate-output tolerance, not a typical bias |
| `imu.accel_noise_std_mps2` | 0.0434 | datasheet | 70 µg/√Hz (TDK lists 65 on X/Y and 70 on Z; the larger is used) = 6.865e-4 m/s²/√Hz, times √4000 Hz |
| `imu.accel_bias_mps2` | [0.0, 0.0, 0.0] | estimated | a zero-g offset belongs to one unit |

## `barometers/opendrone-baro.toml` — OpenDrone barometer (generic)

The number every shipped quad carried inline. No datasheet barometer part ships yet: the noise of real parts depends on
the oversampling mode and the bandwidth, and adding one is a data-only change to a new file under `barometers/`.

| Parameter | Value | Source | Note |
|---|---|---|---|
| `baro.noise_std_pa` | 2.0 | estimated | M1 estimate of the pressure noise per sample (about 0.17 m of height); not measured |

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

## `vtx/opendrone-smartaudio-400.toml` — OpenDrone SmartAudio VTX (400 mW)

| Parameter | Value | Source | Note |
|---|---|---|---|
| `vtx.kind` | "smartaudio" | estimated | SmartAudio v2.1 behavioural model (M3a) |
| `vtx.power_levels_mw` | [25, 100, 200, 400] | estimated | a 400 mW toothpick-class VTX, steps in the style of opendrone-smartaudio-1w; not a datasheet table |
| `vtx.power_levels_dbm` | [14, 20, 23, 26] | estimated | dBm equivalents of the mW table, our own conversion |
| `vtx.reply_latency_ms` | 5.0 | estimated | M3a estimate |
| `vtx.pit_power_mw` | 0.1 | estimated | M3a estimate: a trickle next to the pilot |
| `vtx.antenna.kind` | "omni" | estimated | M3b estimate |
| `vtx.antenna.gain_dbi` | 2.0 | estimated | M3b estimate |
| `vtx.antenna.polarization` | "rhcp" | estimated | M3b estimate |
| `vtx.antenna.mount_frd` | [-0.5, 0.0, -1.0] | estimated | M3b estimate |
| `vtx.thermal.heat_capacity_jpk` | 1.0 | estimated | lumped RC estimate (a gram or two of board and shield); not measured |
| `vtx.thermal.resistance_kpw` | 25.0 | estimated | lumped RC estimate, a small board with little airflow; not measured |
| `vtx.thermal.efficiency` | 0.25 | estimated | RF output over DC input, order of magnitude; not measured |
| `vtx.thermal.idle_w` | 0.3 | estimated | draw with no RF output, order of magnitude; not measured |

