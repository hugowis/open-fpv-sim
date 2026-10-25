# The battery and the thermal model

Two electrical models added in M6b. Both are optional part data that is neutral when absent. **Every constant is an
estimate; none is measured.**

## Per-cell battery

The pack is a **series string of cells** (`crates/ofs-electrical/src/battery.rs`), each a Thevenin equivalent circuit:

    cell voltage  vₖ = OCV(socₖ) − I·R0ₖ − V1ₖ
    dV1ₖ/dt       = I/C1ₖ − V1ₖ/(R1ₖ·C1ₖ)
    pack voltage  = Σ vₖ

The part's `r0_ohm`, `r1_ohm`, `c1_f` and `capacity_mah` are the **pack's** totals, shared out evenly over the `n` cells:
`R0ₖ = r0/n`, `R1ₖ = r1/n`, `C1ₖ = c1·n` and `capₖ = capacity`. Dividing the resistances and multiplying the
capacitance by `n` keeps the polarisation time constant `R1·C1` and makes `ΣV1ₖ` the old single-pack `V1`, so with no
spread the pack voltage is the previous model's to floating-point rounding (the sum is in a different order, so not bit
for bit). Betaflight reads the pack voltage, current and consumed charge, which therefore do not blink across the swap.
All cells carry the same current (they are in series), so they differ only through their own scales.

**Spread.** `cell_r0_scale` and `cell_capacity_scale` multiply a cell's share of `R0` and of the capacity (a tired cell, a
mismatched pack). A cell with half the capacity drains twice as fast, its state of charge reaches 0 first and its voltage
is then the lowest. Each list is empty (all 1.0) or has one positive entry per cell.

**Ageing.** `cycles` with `capacity_fade_per_cycle` and `resistance_growth_per_cycle` (all default 0) give
`capacity_factor = 1 − fade·cycles` (every cell's capacity) and `resistance_factor = 1 + growth·cycles` (every cell's `R0`
only). The loader refuses a part that would leave less than 10 % of the capacity, or whose `resistance_factor` exceeds 10. The factors are linear in the cycle
count: a first approximation.

**The weak-cell fault** (`docs/research/faults.md`) adds its resistance and voltage offsets to the cell it names; with no
cell named it does nothing.

**Published** (recorded like any signal): `battery.cell.{k}.voltage_v`, `battery.cell.{k}.soc`, and the pack's
`battery.voltage_v`, `battery.current_a`, `battery.consumed_mah` (the pack's integrated current) and `battery.soc`
(the mean cell state of charge). The state stream's `battery_cell_voltages` are the cell voltage signals.

| Constant | Value | Provenance |
|---|---|---|
| `cell_r0_scale`, `cell_capacity_scale` | absent in the shipped pack; the aged example has `[1, 1, 1, 1.2, 1, 1]` | estimated |
| `cycles`, `capacity_fade_per_cycle`, `resistance_growth_per_cycle` | 0 in the shipped pack; 150, 0.001, 0.004 in the aged example | estimated: orders of magnitude, not measured |

## Thermal model

One lumped RC node per ESC, motor and video transmitter (`crates/ofs-electrical/src/thermal.rs`), heated by losses the
other models already put on the bus. The part's `thermal` section (motor, ESC, VTX) gives the node; a part without one stays
at the ambient. **Nothing derates**: temperatures are observable only.

Each node is integrated exactly. With heat capacity `C` (J/K), resistance to the ambient `R` (K/W) and power `P` held over
the step `dt`:

    T_ss = T_ambient + P·R          T ← T + (T_ss − T)·(1 − e^(−dt/(R·C)))

so the answer does not depend on the step size, and zero power leaves a node exactly where it is (a quad at rest stays at
the ambient to the last bit). The model steps at 100 Hz after the motors and the battery.

| Node | Power into it |
|---|---|
| ESC `i` | `I²·r_on + k_sw·V·\|I\|`: conduction in the FETs plus switching loss, from the motor's phase current `motor.i.current_a` and the bus voltage. `k_sw ≈ ½·(t_rise + t_fall)·f_sw`, here `0.5 · 100 ns · 48 kHz = 0.0024`. |
| motor `i` | `I²·R_winding + τ_friction·ω`: winding copper loss plus the no-load torque `Ke·I0` that `EscMotor` already takes off the rotor, times the speed. |
| VTX | `P_rf·(1/η − 1) + idle_w` while `vtx.present`; in pit mode only `idle_w` (`vtx.power_mw` keeps the selected level there, and the pit output is a few mW). |

The ambient is `[environment] ambient_c` in the world file (default 25 °C, allowed −40 to 60). The same table carries the earth's magnetic field for the compass (`magnetic_field_ut`, `magnetic_inclination_deg`, `magnetic_declination_deg`; #31).

**Published** (recorded like any signal; the default table records the ESC and motor temperatures at the model's own
100 Hz, `vtx.temp_c` at 50 Hz): `esc.{i}.temp_c`, `motor.{i}.temp_c`, `vtx.temp_c`. A rig that builds the ESC telemetry
or the VTX without a thermal model reads the default ambient (25 °C), not 0 °C. Betaflight sees the ESC
temperatures in the KISS telemetry frames (frame `k` carries ESC `k`, rounded and clamped to 0–255 °C) and the Tramp VTX
temperature in its status reply (a rounded `i16`); with the default 25 °C ambient and no heating they equal the constant
v1 reported.

**Limitations.** The thermal resistance is a constant (no airflow dependence: a quad on the bench cools as it does in a
dive), and with the estimated constants and this model's phase currents the ESC warms only a couple of kelvin where the
winding warms twenty. The point of the model is the structure; the numbers are for the validation framework to replace.

| Constant | Value | Provenance |
|---|---|---|
| ESC `heat_capacity_jpk`, `resistance_kpw`, `r_on_ohm`, `switching_loss_coeff` | 3.0, 8.0, 0.004, 0.0024 | estimated: lumped RC, not measured |
| motor `heat_capacity_jpk`, `resistance_kpw` | 15.0, 1.5 | estimated: lumped RC, not measured |
| VTX `heat_capacity_jpk`, `resistance_kpw`, `efficiency`, `idle_w` | 2.0, 15.0, 0.25, 0.4 | estimated: lumped RC, not measured |
| `ambient_c` | 25.0 | the default; a scenario choice |
