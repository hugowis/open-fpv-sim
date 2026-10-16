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
only). The loader refuses a part that would leave less than 10 % of the capacity. The factors are linear in the cycle
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
