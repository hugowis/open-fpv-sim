# M6b — Battery cells, thermal model and receiver thresholds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace three more v1 simplifications with real models, each with an analytic test and provenance: the receiver's video thresholds as world data (#21), a per-cell battery with capacity fade (#19), and RC thermal networks for the ESCs, motors and VTX (#20).

**Architecture:** Three independent changes in three crates, ordered by risk. (1) `ofs-video` gains a `ReceiverThresholds` struct whose `Default` is today's constants, fed from a new optional `[video]` table of the world file. (2) `ofs-electrical`'s `Battery` becomes a series string of cells, each with its own resistance, polarisation state and state of charge; the weak-cell fault lands on a named cell and the state stream reads real cell voltages. (3) A new `Thermal` model in `ofs-electrical` integrates one lumped RC node per ESC, motor and VTX from the losses already on the bus, and feeds the KISS telemetry frames and Tramp's status reply. Every new knob is optional data that is off (or neutral) when absent.

**Tech Stack:** Rust workspace (`ofs-core`, `ofs-electrical`, `ofs-video`, `ofs-fc`, `ofs-config`, `ofs-sim`), TOML library parts and world files.

**Spec:** There is no dedicated M6 spec. This plan argues from the M6 section of `docs/superpowers/specs/2026-10-13-phase2-real-gear-trusted-numbers-design.md` §4 and the "done when" lines of issues #19, #20 and #21. The modelling calls the issues leave open are in "Design calls" below; they are the plan author's, for review. M6a (#16–#18) is merged (PR #78); M6c (#49 validation harness, #40 repro bundle, phase-close docs) is the next plan.

## Design calls (the author's, not agreed with anyone)

1. **Receiver thresholds live in a world-file `[video]` table** (the issue's wording), not in `[receiver]`. Ten keys, named like the constants they replace: `clean_snr_db`, `sparkle_snr_db`, `sparkle_full_snr_db`, `color_fade_snr_db`, `color_lost_snr_db`, `unstable_snr_db`, `lost_snr_db`, `lost_after_fields`, `relock_after_fields`, `field_drop_p_max`. Absent table or key = today's value, so no existing world file changes and `WORLD_SCHEMA_VERSION` stays. The `pub const`s in `ofs-video/src/link.rs` stay as the documented defaults.
2. **Per-cell model.** Cell `k` of `n`: `R0ₖ = r0_pack/n · scaleₖ · resistance_factor`, polarisation `R1ₖ = r1_pack/n`, `C1ₖ = c1_pack·n` (so the time constant is unchanged and `ΣV1ₖ` equals the old pack `V1`), capacity `capₖ = capacity_mah · cap_scaleₖ · capacity_factor`. Cells are in series, so every cell carries the same current; they differ only through their own scales. Cell voltage `= OCV(socₖ) − I·R0ₖ − V1ₖ`, pack voltage `= Σ cell voltage`. With no spread and no fade the pack numbers equal the old model to floating-point rounding (not bit for bit: the sum is in a different order).
3. **Weak-cell fault.** `fault.battery.weak_cell` (index + 1, 0 = none) now names the cell that carries `fault.battery.r_offset_ohm` (× current) and `fault.battery.v_offset_v` as extra drop. With no cell named the offsets do nothing; `Vehicle::set_fault` always writes all three together, so pack-level behaviour is unchanged. `battery.soc` is the mean cell state of charge; `battery.consumed_mah` is still the pack's integrated current.
4. **Capacity fade is part data.** `cycles` (default 0), `capacity_fade_per_cycle` (default 0), `resistance_growth_per_cycle` (default 0): `capacity_factor = 1 − fade·cycles`, `resistance_factor = 1 + growth·cycles` (applied to `R0` only). The config refuses `capacity_factor < 0.1`. Per-cell spread is `cell_r0_scale` and `cell_capacity_scale` (empty = all 1.0, else one positive entry per cell).
5. **Thermal nodes** are exact exponential RC integrators: `T ← T + (T_ss − T)·(1 − e^{−dt/(R·C)})`, `T_ss = ambient + P·R`, power held over the step. Losses: ESC `P = I²·r_on + k_sw·V·|I|` (phase current `I` from `motor.{i}.current_a`, bus voltage `V`); motor `P = I²·R_winding + τ_friction·ω` (the model's no-load torque `Ke·I0`, which `EscMotor` already applies); VTX `P = P_rf·(1/η − 1) + idle_w` while it is present (`P_rf` from `vtx.power_mw`). No airflow dependence of the thermal resistance, no derating: overtemperature is observable (bus signals, recordings, Betaflight's ESC temperature, Tramp's status), nothing else reacts.
6. **Ambient** is `[environment] ambient_c` in the world file (default 25.0, allowed −40..=60), because it describes where the quad flies, not the quad. A part without a thermal section keeps its temperature at ambient, so with the default 25 °C the KISS and Tramp values are exactly today's constant 25.
7. **KISS temperature** per frame: the `k`-th frame of a period carries ESC `k`'s temperature (`round`, clamped 0..=255). Tramp's status carries the VTX temperature as a rounded `i16`.
8. **The thermal model always exists** (nodes without data just sit at ambient), steps at 100 Hz (`base_hz / 100` divisor), and sits after the battery in the model list so it reads the current tick's motor signals.
9. **Constants shipped** (all `estimated`, none measured): ESC `r_on_ohm 0.004`, `switching_loss_coeff 0.0024`, `heat_capacity_jpk 3.0`, `resistance_kpw 8.0`; motor `heat_capacity_jpk 15.0`, `resistance_kpw 1.5`; both VTX parts `heat_capacity_jpk 2.0`, `resistance_kpw 15.0`, `efficiency 0.25`, `idle_w 0.4`.

## Global Constraints

- Every new constant in a library part carries a `[provenance."<path>"]` entry (a table's leaves are separate paths, e.g. `motor.thermal.heat_capacity_jpk`); none is measured, so all are `source = "estimated"` with a note.
- A new optional parameter that is absent must leave behaviour as before; Task 1 must leave every recording byte-identical (goldens unchanged); Tasks 2 and 3 add recorded channels, so the ELRS radio goldens (`crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`) are regenerated in those two tasks, each OS on its own OS: Windows natively with `OFS_GOLDEN_WRITE=1`, Linux from WSL (`CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test -p ofs-sim --test radio_golden --locked` from `/mnt/c/...`).
- Signals Betaflight reads (pack voltage, current, consumed charge, ESC temperature) must change continuously across the model swap; the OSD must not blink.
- Determinism: no RNG in the battery or thermal models.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass at the end of every task; MSRV is Rust 1.85. `cargo fmt` is not a gate (the repo is not rustfmt-clean; do not run it on existing files).
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Work on a branch in a worktree. Do not tick items in the epic issue #73.
- Tests that run a process-killing WSL case may flake (`ofs-fc` `sitl_errors::a_victim_that_ignores_sigterm_is_sigkilled` fails on untouched `main` under WSL); report it, do not chase it.

## Review Focus

Failure modes the spec implies but a straight reading of the tasks would not test. Each has its test in the named task.

1. A world whose thresholds would divide by zero or invert a ramp (`lost >= unstable`, `sparkle <= 0`, `p_max > 1`) is refused at load, naming the field; an old world file with no `[video]` loads unchanged (Task 1).
2. Battery part lists of the wrong length, zero or negative scales, and a fade that would drive capacity to zero (`cycles·fade ≥ 0.9`) are refused (Task 2).
3. A cell at 0 % state of charge while the others are not: every voltage stays finite and the empty cell is the lowest (Task 2).
4. A weak-cell fault set and then cleared returns the pack to the exact uniform numbers; a fault naming a cell the pack does not have is still refused by `Vehicle::set_fault` (Task 2).
5. Thermal nodes at zero load stay at ambient *exactly* (no drift); a part with no thermal data never leaves ambient however hard it is driven (Task 3).
6. KISS frames for temperatures above 255 °C or below 0 °C clamp instead of wrapping; Tramp encodes a negative temperature as a two's-complement `i16` (Task 3).
7. Thermal parameters with a zero or negative resistance or capacity (which would divide by zero in the time constant) are refused at config load (Task 3).
8. A VTX that is absent, or present but in pit mode with no power, heats only by its idle draw / not at all (Task 3).

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/ofs-video/src/link.rs` | modify | `ReceiverThresholds`, `*_with` functions, `LinkParams.thresholds` |
| `crates/ofs-config/src/world.rs` | modify | `VideoSection`, `EnvironmentSection`, validation |
| `crates/ofs-electrical/src/battery.rs` | modify | the series string of cells |
| `crates/ofs-electrical/src/thermal.rs` | create | `ThermalNode`, `Thermal` model and its params |
| `crates/ofs-electrical/src/lib.rs` | modify | `pub mod thermal;` |
| `crates/ofs-core/src/names.rs` | modify | per-cell, temperature signal names |
| `crates/ofs-config/src/lib.rs`, `part.rs` | modify | battery spread/fade fields, thermal sections, validation |
| `crates/ofs-fc/src/sitl/esc_telemetry.rs` | modify | per-frame temperature from the bus |
| `crates/ofs-video/src/vtx.rs` | modify | Tramp status temperature from the bus |
| `crates/ofs-sim/src/vehicle.rs` | modify | wire all three; `Vehicle::signal`; real cell voltages in `state()` |
| `library/batteries/opendrone-6s-1300-aged.toml` | create | aged-pack example part |
| `library/{escs,motors,vtx}/*.toml` | modify | thermal data + provenance |
| `docs/research/battery-and-thermal.md` | create | the two models, constants tables |
| `docs/research/video-link.md`, `faults.md`, `component-library.md`, `docs/roadmap.md`, `CHANGELOG.md` | modify | reflect the three models |

---

### Task 1: Receiver thresholds as world data (#21)

**Files:**
- Modify: `crates/ofs-video/src/link.rs`, `crates/ofs-config/src/world.rs`, `crates/ofs-sim/src/vehicle.rs` (`link_params`)
- Test: `crates/ofs-video/tests/link.rs`, `crates/ofs-config/tests/world.rs`, create `crates/ofs-sim/tests/video_thresholds.rs`
- Modify: `docs/research/video-link.md`, `docs/roadmap.md`, `CHANGELOG.md`

**Interfaces:**
- Consumes: the existing consts `CLEAN_SNR_DB … FIELD_DROP_P_MAX` and functions `picture`, `field_drop_probability`, struct `SyncTracker` in `link.rs`.
- Produces:
  - `ofs_video::link::ReceiverThresholds { clean_snr_db, sparkle_snr_db, sparkle_full_snr_db, color_fade_snr_db, color_lost_snr_db, unstable_snr_db, lost_snr_db: f64, lost_after_fields: u32, relock_after_fields: u32, field_drop_p_max: f64 }` (`Debug, Clone, Copy, PartialEq`), whose `Default` is the consts.
  - `pub fn picture_with(t: &ReceiverThresholds, snr_db: f64) -> Picture`, `pub fn field_drop_probability_with(t: &ReceiverThresholds, snr_db: f64) -> f64`, `SyncTracker::with_thresholds(t: ReceiverThresholds) -> SyncTracker`; `picture`, `field_drop_probability` and `SyncTracker::new()` stay and use the defaults.
  - `LinkParams.thresholds: ReceiverThresholds`.
  - `ofs_config::world::VideoSection` (same ten field names and types, `Default` = the same numbers, `#[serde(default)]` per field and on the table) and `WorldConfig.video: VideoSection` with `#[serde(default)]`.
  - `ofs_sim::vehicle::receiver_thresholds(v: &VideoSection) -> ReceiverThresholds`.

- [ ] **Step 1: Write the failing video tests** in `crates/ofs-video/tests/link.rs` (it has `close`, `bare`, `open_field` helpers): add `thresholds: ReceiverThresholds::default()` to the `bare` literal (it will not compile until Step 3) and these tests:
  - `the_default_thresholds_are_the_published_constants`: each field of `ReceiverThresholds::default()` equals its const; for `snr` from −5.0 to 35.0 in steps of 0.5, `picture_with(&d, snr) == picture(snr)` and `field_drop_probability_with(&d, snr) == field_drop_probability(snr)` (exact).
  - `a_raised_clean_threshold_adds_grain_to_a_strong_signal`: with `clean_snr_db: 40.0`, `picture_with(&t, 30.0).noise > 0.0` while `picture(30.0).noise == 0.0`.
  - `a_raised_lost_threshold_loses_sync_on_a_weaker_signal`: `t = { unstable_snr_db: 12.0, lost_snr_db: 9.0, ..default }`; a `SyncTracker::with_thresholds(t)` fed `8.0` four times ends `VideoSync::Lost`, a default tracker fed `8.0` four times stays `Locked`.
  - `one_bad_field_loses_sync_when_lost_after_fields_is_one`: `lost_after_fields: 1`, one update at 2.0 dB after one at 20.0 → `Lost`; the default needs three.
  - `a_zero_drop_probability_never_drops_a_field`: `field_drop_p_max: 0.0` → `field_drop_probability_with(&t, 4.5) == 0.0`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-video --test link --locked`
Expected: compile error (`ReceiverThresholds` undefined).

- [ ] **Step 3: Implement in `link.rs`.** Add `ReceiverThresholds` with a manual `Default` from the consts. `picture_with`, `field_drop_probability_with` and `SyncTracker` use the passed values wherever they use a const today (`SyncTracker` gains a `thresholds` field; `update` reads it; `Default` still derives). `picture`/`field_drop_probability` delegate with `ReceiverThresholds::default()`. `LinkParams` gains `pub thresholds: ReceiverThresholds`; `VideoLink::new` builds its tracker with `SyncTracker::with_thresholds(params.thresholds)` and the per-field code calls the `_with` functions with it. Keep the consts' doc comments.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-video --locked`
Expected: PASS (existing tests unchanged: they use the defaults).

- [ ] **Step 5: Write the failing config tests** in `crates/ofs-config/tests/world.rs` (helpers `world_text`, `problems`, `written_world`):
  - `a_world_without_a_video_table_gets_the_default_thresholds`: `world::load(WORLD).unwrap().video == VideoSection::default()` and `WorldConfig::open_field().video == VideoSection::default()`.
  - `a_video_table_overrides_only_the_keys_it_names`: `written_world(&(world_text() + "\n[video]\nunstable_snr_db = 8.0\n"))` has `unstable_snr_db == 8.0`, every other field default.
  - `video_threshold_problems_are_reported_together`: `problems(&(world_text() + "\n[video]\nlost_snr_db = 7.0\nsparkle_snr_db = 0.0\ncolor_lost_snr_db = 9.0\nfield_drop_p_max = 1.5\nlost_after_fields = 0\n"))` mentions each of `video.unstable_snr_db`, `video.sparkle_snr_db`, `video.color_fade_snr_db`, `video.field_drop_p_max`, `video.lost_after_fields`. The rules: all finite; `sparkle_snr_db > 0`; `clean > sparkle > sparkle_full`; `color_fade > color_lost`; `unstable > lost`; both counts `>= 1`; `field_drop_p_max` in `[0, 1]`; an ordering violation names the *larger* field.

- [ ] **Step 6: Run to verify they fail**, then **implement `VideoSection` and the validation** in `world.rs` (field defaults as small `fn`s or one `Default` impl with `#[serde(default)]` on the struct; `deny_unknown_fields`; `WorldConfig.video`; `open_field()` sets `video: VideoSection::default()`; the checks in `WorldConfig::validate`). Run `cargo test -p ofs-config --locked` → PASS.

- [ ] **Step 7: Wire into the vehicle and test.** In `crates/ofs-sim/src/vehicle.rs` add `pub fn receiver_thresholds(v: &VideoSection) -> ReceiverThresholds` and set `thresholds: receiver_thresholds(&world.video)` in `link_params`. Create `crates/ofs-sim/tests/video_thresholds.rs` (helpers as in `open_loop_vehicle.rs`: `load` the freestyle quad, `FcKind::OpenLoop`, a mutable `WorldConfig::open_field()`): `the_default_world_thresholds_equal_the_links_defaults` (`receiver_thresholds(&VideoSection::default()) == ReceiverThresholds::default()`), and `a_stricter_clean_threshold_changes_the_flight_record` (run 0.3 s with `world.video.clean_snr_db = 60.0` and again with the default: digests differ; and a third run with the default world equals the second).
Run: `cargo test -p ofs-sim --test video_thresholds --locked` → PASS.

- [ ] **Step 8: Document.** In `docs/research/video-link.md` document the `[video]` table beside the existing threshold constants (keys, defaults, the validation rules, that each is an estimate). Remove "The receiver's thresholds are named constants, not world-file fields;" from `docs/roadmap.md`'s Video list and the "Configurable receiver thresholds" backlog item. Add a `## Unreleased — M6b: battery, thermal and receiver` heading to `CHANGELOG.md` with an "Added" bullet for `[video]`.

- [ ] **Step 9: Run everything.** `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and from WSL `CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test --workspace --locked --no-fail-fast`. Expected: all PASS and `git status` shows **no** change to `tests/golden/` (the defaults reproduce the old behaviour exactly).

- [ ] **Step 10: Commit**

```bash
git add crates docs CHANGELOG.md
git commit -m "feat(video): receiver thresholds as world data (#21)"
```

---

### Task 2: A per-cell battery with capacity fade (#19)

**Files:**
- Modify: `crates/ofs-core/src/names.rs`, `crates/ofs-electrical/src/battery.rs`, `crates/ofs-config/src/lib.rs` (`BatterySection`, `validate`, the battery-part merge near `BatterySection {` in the loader), `crates/ofs-config/src/part.rs` (`BatteryHardware`), `crates/ofs-sim/src/vehicle.rs` (`build`, `state`, handles)
- Create: `library/batteries/opendrone-6s-1300-aged.toml`, `docs/research/battery-and-thermal.md`
- Test: `crates/ofs-electrical/tests/battery.rs`, `crates/ofs-config/tests/{config,part}.rs`, create `crates/ofs-sim/tests/battery_cells.rs`; the existing `vehicle.rs` unit tests around the cell-voltage assertions
- Modify: `docs/research/faults.md` ("Weak cell"), `docs/research/component-library.md`, `docs/roadmap.md`, `CHANGELOG.md`; regenerate the radio goldens

**Interfaces:**
- Consumes: Task 1 nothing. `fault.battery.{r_offset_ohm, v_offset_v, weak_cell}` signals as today.
- Produces:
  - `names::battery_cell_voltage(i: usize) -> String` = `"battery.cell.{i}.voltage_v"`, `names::battery_cell_soc(i: usize) -> String` = `"battery.cell.{i}.soc"`.
  - `BatteryParams` gains `cell_r0_scale: Vec<f64>`, `cell_capacity_scale: Vec<f64>` (empty = all 1.0), `capacity_factor: f64`, `resistance_factor: f64` (1.0 = neutral).
  - `BatteryHardware` / `BatterySection` gain `cell_r0_scale: Vec<f64>`, `cell_capacity_scale: Vec<f64>`, `cycles: u32`, `capacity_fade_per_cycle: f64`, `resistance_growth_per_cycle: f64`, each `#[serde(default)]`.
  - `Vehicle::signal(&self, name: &str) -> Option<f64>` (reads a scalar bus signal; for tests and tools).

- [ ] **Step 1: Write the failing battery tests** in `crates/ofs-electrical/tests/battery.rs`. Extend its `params()` with `cell_r0_scale: vec![]`, `cell_capacity_scale: vec![]`, `capacity_factor: 1.0`, `resistance_factor: 1.0` (the file's existing tests must keep passing). New tests (6 cells, `r0 = 0.024`, so a cell's `R0 = 0.004`):
  - `uniform_cells_reproduce_the_pack_model`: 10 A for 20 s; the pack voltage equals `6·OCV(soc) − I·R0_pack − I·R1·(1 − e^{−t/τ})` within 1e-9 with `soc = 1 − I·t/(cap·3.6)`, `τ = R1·C1`; and every cell voltage equals `pack/6` within 1e-9.
  - `the_pack_is_the_series_sum_of_its_cells`: `cell_r0_scale = [1,1,1,1,1,2]`; after one step at 10 A, cell `k` voltage equals `OCV(1.0) − 10·0.004·scaleₖ` (within 1e-12) and `battery.voltage_v` equals their sum.
  - `a_smaller_cell_drains_faster`: `cell_capacity_scale = [0.5,1,1,1,1,1]`; after 100 s at 10 A cell 0's soc is `1 − 10·100/(1300·0.5·3.6)` and the others `1 − 10·100/(1300·3.6)` (within 1e-9); `battery.soc` is their mean; `battery.consumed_mah` is the pack's, unchanged by the spread.
  - `fade_and_growth_scale_the_pack`: `capacity_factor: 0.8` drains soc `1/0.8` as fast; `resistance_factor: 1.5` deepens the first-step sag by exactly 1.5× the ohmic part.
  - `the_weak_cell_fault_lands_on_the_named_cell`: set `weak_cell = 3.0`, `r_offset 0.02`, `v_offset 0.1`; at 10 A cell 2 is lower than a clean pack's cell 2 by `10·0.02 + 0.1` and every other cell is bit-equal to the clean pack's; with `weak_cell = 0.0` and the same offsets nothing changes; clearing (all three back to 0) restores the clean voltages exactly.
  - `an_empty_cell_stays_finite`: `cell_capacity_scale = [0.001,1,1,1,1,1]`, 10 A for 5 s: all cell voltages finite, cell 0's soc is `0.0` and its voltage the lowest.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-electrical --test battery --locked`
Expected: compile error (new `BatteryParams` fields).

- [ ] **Step 3: Implement the cell string in `battery.rs`** per Design call 2–4: per-cell `soc: Vec<f64>`, `v1: Vec<f64>`, `r0: Vec<f64>` (computed at `new`), `cap_mah: Vec<f64>`; `new` panics (like the table check) when a non-empty scale list's length differs from `cells`. `step` updates every cell from the same pack current; `publish` writes `battery.voltage_v` = Σ cell voltage, the per-cell `battery.cell.{k}.voltage_v` and `.soc`, `battery.soc` = mean soc, and reads the three fault signals each publish. Add the two name functions.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-electrical --locked`
Expected: PASS, including the pre-existing battery tests.

- [ ] **Step 5: Config.** Add the five fields to `BatteryHardware` and `BatterySection` (copied through in the loader's battery merge) and, in `validate`: each non-empty scale list has `cells` entries, each finite and `> 0` (`battery.cell_r0_scale`, `battery.cell_capacity_scale`); `capacity_fade_per_cycle` finite in `[0, 1)`; `resistance_growth_per_cycle` finite `>= 0`; and `1 − fade·cycles >= 0.1` reported on `battery.cycles`. Tests: in `crates/ofs-config/tests/config.rs`, `battery_spread_and_fade_problems_are_reported` (wrong length, a zero scale, `fade 0.01` with `cycles 120`, each naming its field, using the file's `write_quad`/`write_part` on the shipped battery part text) and `the_shipped_battery_has_no_spread_or_fade` (`cell_*_scale` empty, `cycles == 0`); in `crates/ofs-config/tests/part.rs`, `a_battery_part_may_carry_spread_and_fade` (loads with provenance for each present key) and `a_spread_key_without_provenance_is_refused`. Run `cargo test -p ofs-config --locked` → PASS.

- [ ] **Step 6: Wire into the vehicle.** In `build`, pass the new fields to `BatteryParams` (`capacity_factor = 1 − fade·cycles`, `resistance_factor = 1 + growth·cycles`). Add `Vehicle::signal`. In `state()` replace the synthesised `battery_cell_voltages` with the real `battery.cell.{k}.voltage_v` signals (handles registered in `VehicleHandles`), and delete what becomes unused (the droop-share arithmetic; keep `weak_cell_index` if `set_fault` still uses it). Update the existing `vehicle.rs` unit tests that assert cell voltages only if their tolerance proves too tight for the new rounding (keep the intent: equal shares when clean, the weak cell lower by the fault's drop). Create `crates/ofs-sim/tests/battery_cells.rs` (freestyle quad, open loop): `a_clean_pack_has_six_equal_cells` (`state().battery_cell_voltages` six entries within 1e-9 of `pack/6`), `an_aged_pack_sags_more_in_flight` (mutate `cfg.battery.cycles = 200`, `capacity_fade_per_cycle = 0.001`, `resistance_growth_per_cycle = 0.004`; full throttle 1 s; pack voltage lower than the fresh pack's and consumed-capacity ratio matches), `a_weak_cell_fault_lowers_exactly_that_cell` (`Fault::WeakCell` on cell 3, then cleared: the cell returns to equal), and `vehicle_signal_reads_the_bus` (`Vehicle::signal("battery.voltage_v")` equals `state().battery_voltage_v`; an unknown name is `None`).

- [ ] **Step 7: Aged example part.** Create `library/batteries/opendrone-6s-1300-aged.toml`: the shipped pack plus `cell_r0_scale = [1.0, 1.0, 1.0, 1.2, 1.0, 1.0]`, `cycles = 150`, `capacity_fade_per_cycle = 0.001`, `resistance_growth_per_cycle = 0.004`, each with `source = "estimated"` provenance ("a worn pack with one tired cell; not measured"). A test in `battery_cells.rs`, `the_aged_library_pack_loads`, loads it with `part::load_battery`.

- [ ] **Step 8: Run the vehicle tests, then document.**

Run: `cargo test -p ofs-sim --locked`
Expected: PASS except the radio golden (next step).
Create `docs/research/battery-and-thermal.md` with a "Per-cell battery" section (the cell equations, why `ΣV1ₖ` equals the old pack `V1`, the fade formulas, the weak-cell mapping, what is published, constants table: spread/fade keys, `estimated`). Rewrite `docs/research/faults.md` "Weak cell": cell voltages are now real, the additive terms land on the named cell, the equal-share simplification is gone. Add the new keys and the aged part to `docs/research/component-library.md`. Delete "The battery is one Thevenin pack…" from `docs/roadmap.md` and the "per-cell battery with capacity fade" backlog mention. Add `CHANGELOG.md` entries (Added: per-cell battery, fade, aged part, `battery.cell.{k}.*` channels; Changed: goldens regenerated).

- [ ] **Step 9: Regenerate the goldens and run everything.**

Run (PowerShell): `$env:OFS_GOLDEN_WRITE=1; cargo test -p ofs-sim --test radio_golden --locked; Remove-Item Env:OFS_GOLDEN_WRITE`, then from WSL in the worktree `OFS_GOLDEN_WRITE=1 CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test -p ofs-sim --test radio_golden --locked`; then clippy and the full Windows and WSL suites as in Task 1 Step 9.
Expected: both digest files changed; everything PASSES on both OSes.

- [ ] **Step 10: Commit**

```bash
git add crates library docs CHANGELOG.md
git commit -m "feat(electrical): a per-cell battery with capacity fade (#19)"
```

---

### Task 3: Thermal model for ESCs, motors and VTX (#20)

**Files:**
- Create: `crates/ofs-electrical/src/thermal.rs`
- Modify: `crates/ofs-electrical/src/lib.rs`, `crates/ofs-core/src/names.rs`, `crates/ofs-config/src/lib.rs` (`MotorSection`, `EscSection`, the VTX section merge, `validate`), `crates/ofs-config/src/part.rs` (`VtxHardware`), `crates/ofs-config/src/world.rs` (`EnvironmentSection`), `crates/ofs-fc/src/sitl/esc_telemetry.rs`, `crates/ofs-video/src/vtx.rs`, `crates/ofs-sim/src/vehicle.rs`, `library/escs/opendrone-4in1-esc.toml`, `library/motors/opendrone-openmotor-2207.toml`, `library/vtx/*.toml`
- Test: create `crates/ofs-electrical/tests/thermal.rs`, `crates/ofs-sim/tests/thermal.rs`; modify `crates/ofs-fc/tests/esc_telemetry.rs`, `crates/ofs-video/tests/vtx.rs`, `crates/ofs-config/tests/{config,part,world}.rs`
- Modify: `docs/research/battery-and-thermal.md`, `docs/research/component-library.md`, `docs/roadmap.md`, `CHANGELOG.md`; regenerate the radio goldens

**Interfaces:**
- Consumes: Task 2's `Vehicle::signal`; existing `motor.{i}.current_a`, `motor.{i}.omega_radps`, `battery.voltage_v`, `vtx.power_mw`, `vtx.present`.
- Produces:
  - `names::esc_temp(i: usize)` = `"esc.{i}.temp_c"`, `names::motor_temp(i: usize)` = `"motor.{i}.temp_c"`, `names::VTX_TEMP_C` = `"vtx.temp_c"`.
  - In `ofs_electrical::thermal`: `NodeParams { heat_capacity_jpk: f64, resistance_kpw: f64 }`; `EscThermalParams { node: NodeParams, r_on_ohm: f64, switching_loss_coeff: f64 }`; `MotorThermalParams { node: NodeParams, winding_resistance_ohm: f64, friction_torque_nm: f64 }`; `VtxThermalParams { node: NodeParams, efficiency: f64, idle_w: f64 }`; `ThermalParams { ambient_c: f64, esc: Vec<Option<EscThermalParams>>, motor: Vec<Option<MotorThermalParams>>, vtx: Option<VtxThermalParams> }` (all `Debug, Clone`); `ThermalNode::new(p: NodeParams, start_c: f64) -> ThermalNode`, `ThermalNode::step(&mut self, power_w: f64, ambient_c: f64, dt_s: f64) -> f64` (returns the new temperature); `Thermal::new(p: ThermalParams, rate_divisor: u32, bus: &mut Bus) -> Thermal` (a `Model` named `"thermal"`).
  - Config: `NodeSection`-style optional `thermal` sections: `motor.thermal { heat_capacity_jpk, resistance_kpw }`, `esc.thermal { heat_capacity_jpk, resistance_kpw, r_on_ohm, switching_loss_coeff }`, `vtx.thermal { heat_capacity_jpk, resistance_kpw, efficiency, idle_w }`; `ofs_config::world::EnvironmentSection { ambient_c: f64 }` and `WorldConfig.environment` (`#[serde(default)]`, default 25.0).

- [ ] **Step 1: Write the failing thermal tests** in `crates/ofs-electrical/tests/thermal.rs`:
  - `a_node_follows_the_rc_step_response`: `C = 2 J/K`, `R = 10 K/W` (τ = 20 s), 5 W, ambient 25, start 25: after 20 s of 0.01 s steps the temperature is `25 + 50·(1 − e^{−1})` within 1e-9; after 400 s it is `75` within 1e-6.
  - `the_step_size_does_not_change_the_answer`: one 20 s step equals two thousand 0.01 s steps within 1e-9.
  - A scheduler helper holding a `Thermal` (divisor 80 at 8 kHz) with bus signals `motor.0.current_a`, `motor.0.omega_radps`, `battery.voltage_v`, `vtx.power_mw`, `vtx.present` set by the test:
  - `an_esc_settles_at_the_analytic_temperature`: `r_on 0.004`, `k_sw 0.0024`, `C 3`, `R 8`, ambient 25, 20 A, 24 V: `P = 400·0.004 + 0.0024·24·20 = 2.752 W`, `T_ss = 25 + 2.752·8`; after 300 s the signal `esc.0.temp_c` is within 1e-3.
  - `a_motor_settles_at_the_analytic_temperature`: winding `0.07 Ω`, friction `0.005 N·m`, `ω = 1000 rad/s`, 20 A, `C 15`, `R 1.5`: `P = 28 + 5 W`; `T_ss = 25 + 33·1.5`.
  - `the_vtx_heats_by_its_dissipation`: `vtx.present = 1`, 1000 mW, `η 0.25`, `idle 0.4`, `C 2`, `R 15`: `P = 1·(4 − 1) + 0.4`; `T_ss = 25 + 3.4·15`.
  - `idle_stays_exactly_at_ambient`: zero current, speed and VTX power for 60 s: every temperature signal `==` ambient (exact equality), for ambient 25 and 40.
  - `a_part_without_thermal_data_never_leaves_ambient`: `esc: vec![None]`, `motor: vec![None]`, 40 A: the signals stay at ambient exactly.
  - `an_absent_vtx_stays_at_ambient`: `vtx: Some(..)` but `vtx.present = 0`: `vtx.temp_c == ambient`.
  - `a_vtx_with_no_output_power_heats_only_by_its_idle_draw`: `vtx.present = 1`, `vtx.power_mw = 0`, `idle 0.4`, `C 2`, `R 15`: `T_ss = 25 + 0.4·15`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p ofs-electrical --test thermal --locked`
Expected: compile error (`thermal` module missing).

- [ ] **Step 3: Implement `thermal.rs`** (and `pub mod thermal;`, the three names). `ThermalNode::step` is the exact integrator of Design call 5 (`temp += (ambient + P·R − temp)·(1 − exp(−dt/(R·C)))`). `Thermal` registers all `esc.{i}.temp_c` / `motor.{i}.temp_c` for every index of the param vectors and `vtx.temp_c`, publishes ambient at construction, and on each step computes the losses of Design call 5 from the bus (ESC and motor losses from the same index's signals; the VTX loss only while `vtx.present > 0.5`), steps the nodes that exist and republishes; a missing node publishes `ambient_c` unchanged. It uses `ctx.dt_s` as its own period, as `Battery` does.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p ofs-electrical --locked`
Expected: PASS.

- [ ] **Step 5: Telemetry consumers (tests first).** In `crates/ofs-fc/tests/esc_telemetry.rs`, following the file's existing pattern for building the model and reading frames: set `esc.k.temp_c` for four motors to `30.4, 61.6, 300.0, -5.0` and assert the four frames of one period carry `30, 62, 255, 0` in byte 0 (and that frames still carry the CRC the file already checks); the other existing tests set all four temperatures to `25.0` where they assert the old constant. In `crates/ofs-video/tests/vtx.rs` replace the `25 "the constant temperature"` assertion at the Tramp status test with one that sets `vtx.temp_c = 47.4` before the query and expects `47`, and add `tramp_status_encodes_a_negative_temperature`: `-5.6` → `i16::from_le_bytes(reply[6..8]) == -6`. Run both: they fail (the constant is still compiled in). Then implement: `EscTelemetry` reads `names::esc_temp(k)` for frame `k` (replacing `TEMPERATURE_C`; remove the const), converting with `round` and a clamp to `0.0..=255.0`; `VtxModel`'s 's' reply reads `names::VTX_TEMP_C` and writes `(t.round() as i16).to_le_bytes()` where it wrote `25u16.to_le_bytes()`, and its doc comment drops "25 °C is the constant". Run `cargo test -p ofs-fc -p ofs-video --locked` → PASS.

- [ ] **Step 6: Config (tests first).** Add the optional `thermal` sections to `MotorSection`, `EscSection` and `VtxHardware`/the quad's VTX section (copy through the loader the same way `antenna` and `pit_power_mw` are), `EnvironmentSection` to the world, and in `validate`: every `heat_capacity_jpk` and `resistance_kpw` finite `> 0`, `r_on_ohm` and `switching_loss_coeff` and `idle_w` finite `>= 0`, `efficiency` finite in `(0, 1]`, `environment.ambient_c` finite in `[-40, 60]` (field names like `motors[0].thermal.resistance_kpw`, `escs[0].thermal.r_on_ohm`, `vtx.thermal.efficiency`, `environment.ambient_c`). Tests: in `crates/ofs-config/tests/config.rs`, `thermal_problems_are_reported_together` (a zero resistance on the motor part, an efficiency of 1.5 on the VTX part, a negative `r_on_ohm` on the ESC part, each field named) and `the_shipped_parts_carry_thermal_data` (after Step 8); in `part.rs`, `a_thermal_section_without_provenance_is_refused`; in `world.rs`, `the_ambient_defaults_to_25_and_is_range_checked`. Run `cargo test -p ofs-config --locked`; implement until PASS.

- [ ] **Step 7: Wire into the vehicle (tests first).** Create `crates/ofs-sim/tests/thermal.rs` (freestyle quad, open loop, `Vehicle::signal`): 
  - `a_quad_at_rest_stays_at_ambient`: 30 s idle: `esc.0.temp_c`, `motor.0.temp_c` and `vtx.temp_c` all equal 25.0 exactly.
  - `full_throttle_heats_the_esc_and_the_motor`: 20 s at full throttle: both rise at least 5 K above ambient, the motor more than the ESC is *not* asserted (constants are estimates); after the throttle is cut and 60 s pass, both are lower than their peak.
  - `a_warmer_world_starts_warmer`: `world.environment.ambient_c = 35.0`: an idle quad's temperatures are exactly 35.0.
  - `the_hot_esc_reaches_the_kiss_frame`: the vehicle's recorded `esc.0.temp_c` after 20 s full throttle matches what `EscTelemetry` would clamp (assert `Vehicle::signal("esc.0.temp_c")` is in `(30.0, 255.0)`; the frame path itself is covered in Step 5).
  Run (fails: no thermal model). Then in `build`, after the `Battery` is pushed, build `ThermalParams` from `cfg` (per-motor and per-ESC `Option`s from the sections; `winding_resistance_ohm = motor.resistance_ohm`, `friction_torque_nm = Ke·I0` with `Ke = 60/(2π·kv)`; the VTX node from the quad's VTX section; `ambient_c = opts.world.environment.ambient_c`) and `models.push(Box::new(Thermal::new(params, (base_hz / 100).max(1), &mut bus)))`. Run `cargo test -p ofs-sim --test thermal --locked` → PASS.

- [ ] **Step 8: Ship the constants.** Add the `thermal` data of Design call 9 with provenance (`source = "estimated"`, note "lumped RC estimate; not measured") to `library/escs/opendrone-4in1-esc.toml`, `library/motors/opendrone-openmotor-2207.toml`, and both `library/vtx/*.toml`. Run `cargo test -p ofs-config -p ofs-sim --locked` → PASS.

- [ ] **Step 9: Document.** Extend `docs/research/battery-and-thermal.md` with "Thermal model": the node equation and why the exact integrator is used, the three loss formulas with the switching-loss reasoning (`≈ ½·(t_r+t_f)·f_sw·V·I`, here `0.5·100 ns·48 kHz = 0.0024`), the ambient, what is published and who reads it, that nothing derates, the limitation that thermal resistance does not depend on airflow, and the constants table (all `estimated`). Update `docs/research/component-library.md` for the new keys, `docs/roadmap.md` (remove "ESC and VTX temperatures are a constant 25 °C…" and the thermal backlog item), and `CHANGELOG.md` (Added: the thermal model, `[environment]`, the temperature channels; Changed: KISS and Tramp now report simulated temperatures; goldens regenerated).

- [ ] **Step 10: Regenerate the goldens and run everything**, exactly as Task 2 Step 9. Expected: both digest files change (new `*.temp_c` channels); all PASS on both OSes.

- [ ] **Step 11: Commit**

```bash
git add crates library docs CHANGELOG.md
git commit -m "feat(electrical): RC thermal model for ESCs, motors and VTX (#20)"
```

---

### Task 4: Wrap-up and hand-off

**Files:**
- Modify: `docs/roadmap.md` (the M6 status line)

- [ ] **Step 1: MSRV check.** `cargo +1.85 check --workspace --all-targets --locked --exclude ofs-godot`. Expected: no error.

- [ ] **Step 2: Whole-branch verification.** The full command set of Task 3 Step 10 on a clean tree, plus `python -m pytest python/tests` **from the repository root** (a relative `quads/…` path in those tests resolves against the working directory). Expected: all PASS (SITL tests skip without `OFS_SITL_LAUNCH`).

- [ ] **Step 3: Live-SITL check.** From WSL: `CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf ~/.cargo/bin/cargo test -p ofs-fc -p ofs-sim --test sitl_live -- --ignored --test-threads=1`. Expected: PASS (10 tests). This is the check that Betaflight still reads the pack voltage, current, consumption and the new per-ESC temperature from the KISS frames (`esc_telemetry_frames_become_battery_state`).

- [ ] **Step 4: Roadmap status and commit.** Update the M6 paragraph under "Post-v1 backlog" in `docs/roadmap.md`: M6a and M6b done, M6c (validation harness, repro bundle) next.

```bash
git add docs/roadmap.md
git commit -m "docs: M6b done - per-cell battery, thermal model, receiver thresholds"
```

- [ ] **Step 5: Report to the maintainer** in the PR description: the three models and their constants (all estimated), that two goldens changed per OS and why (new channels, in Tasks 2 and 3), that Task 1 left them untouched, the live-SITL result, that #19–#21 can be ticked in #73 on merge, and the deferred minors. Then write the M6c plan.
