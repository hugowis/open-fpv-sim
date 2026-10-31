# Procedural flight audio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Live procedural flight audio in the Godot client (motor tones, prop wash, wind, collision thud) from a pure, Godot-free synthesis core, with a persisted on/off setting.

**Architecture:** A new dependency-free crate `ofs-audio` holds the pure function `params(AudioInput) -> AudioParams`, a deterministic `Synth` and an `Audio` facade. `ofs-client`'s `Telemetry` gains `motor_rpm`; `ofs-godot`'s `OfsClient` owns an `Audio` and exposes `audio_render`; a Godot node pushes the rendered frames into an `AudioStreamGenerator`. No protocol change (stays 15).

**Tech Stack:** Rust (workspace crates), godot-rust 0.5 (`ofs-godot`), GDScript (Godot 4.7.2).

**Spec:** `docs/superpowers/specs/2026-10-31-flight-audio-design.md`

## Global Constraints

- `ofs-audio` has no dependencies, no I/O, no Godot, no tokio; no `ofs-sim`-side crate (`ofs-core`, `ofs-config`, `ofs-physics`, `ofs-electrical`, `ofs-sensors`, `ofs-rf`, `ofs-radio`, `ofs-fc`, `ofs-video`, `ofs-log`, `ofs-proto`, `ofs-sim`) may depend on it.
- Protocol stays 15; do not touch `proto/`, `crates/ofs-proto`, or the Python client.
- `SAMPLE_RATE` = 44100, defined once in `ofs-audio`; Godot reads it through `OfsClient.audio_sample_rate()`.
- Constants (`AudioConfig` defaults): blades 3; motor_gain 0.5; wash descent reference 5 m/s; wind gain reference 25 m/s, exponent 1.5; wind cutoff 300 Hz at rest to 3500 Hz at 30 m/s and above; thud reference 12 m/s, exponent 0.7, thud sine 90 Hz decaying ~120 ms; slew ~20 ms.
- Do not touch `crates/ofs-physics`, `crates/ofs-config/src/world.rs`, `crates/ofs-core/src/shape.rs`, `worlds/` (another session works there).
- Files stay LF: check `tr -cd '\r' < file | wc -c` prints 0 for every file touched.
- Rust tests run in WSL (see `docs/dev-setup.md`); Godot tests via `bash scripts/run-godot-tests.sh` with `OFS_GODOT_DL=/mnt/c/dev/open-fpv-sim/build/godot-dl` and `OFS_SIM_BIN` pointing at the debug build; read the WHOLE Godot output (it prints `0 failed` even after a SCRIPT ERROR).
- Commit locally after each task; never push, never open a PR.
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.

## Review Focus

- A non-finite or negative rpm/cmd/airspeed, or a NaN anywhere: silence, never NaN samples. (Task 1, Task 2)
- `motor_rpm` and `motor_cmd` of different length, or no motors: use the shorter length, no panic. (Task 1)
- `render` with a zero-length buffer, and a very large rpm: no panic, output stays in [-1, 1]. (Task 2)
- A collision event whose state never arrives, or a reload (`reset`) with a thud pending: no stale thud after the reload. (Task 2)
- Audio off or session paused: no frames pushed, bus muted, and switching back on does not play a stale burst. (Task 4)

---

### Task 1: `ofs-audio` crate, the pure parameters

**Files:**
- Create: `crates/ofs-audio/Cargo.toml`, `crates/ofs-audio/src/lib.rs`, `crates/ofs-audio/src/params.rs`, `crates/ofs-audio/tests/params.rs`, `crates/ofs-audio/tests/isolation.rs`
- Modify: `Cargo.toml` (workspace dependency `ofs-audio = { path = "crates/ofs-audio" }`), `Cargo.lock` (by cargo)

**Interfaces:**
- Produces (all `pub`, re-exported from `lib.rs`):
  - `pub const SAMPLE_RATE: u32 = 44100;`
  - `#[derive(Clone, Debug, PartialEq)] pub struct AudioConfig { pub blades: f64, pub motor_gain: f64, pub wash_descent_ref_mps: f64, pub wind_ref_mps: f64, pub wind_exponent: f64, pub wind_cutoff_min_hz: f64, pub wind_cutoff_max_hz: f64, pub wind_cutoff_full_mps: f64, pub thud_ref_mps: f64, pub thud_exponent: f64 }` with `Default` giving the Global Constraints values.
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct AudioInput { pub motor_rpm: Vec<f64>, pub motor_cmd: Vec<f64>, pub airspeed_mps: f64, pub descent_mps: f64, pub active: bool }`
  - `#[derive(Clone, Copy, Debug, Default, PartialEq)] pub struct MotorTone { pub freq_hz: f64, pub gain: f64 }`
  - `#[derive(Clone, Copy, Debug, Default, PartialEq)] pub struct Wind { pub gain: f64, pub cutoff_hz: f64 }`
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct AudioParams { pub motors: Vec<MotorTone>, pub wash_gain: f64, pub wind: Wind }`
  - `pub fn params(input: &AudioInput, cfg: &AudioConfig) -> AudioParams`
  - `pub fn thud_gain(collision_speed_mps: f64, cfg: &AudioConfig) -> f64`

- [ ] **Step 1: Write the failing tests** in `tests/params.rs` (use `AudioConfig::default()`; a helper `input(rpm, cmd, speed, descent)` builds an active input):
  - `same_input_same_params`: `params(&i, &c) == params(&i, &c)` for a 4-motor input.
  - `motor_pitch_is_blade_pass_of_rpm`: rpm 30000, 3 blades, cmd 0.5 -> `motors[0].freq_hz == 1500.0`; rpm 15000 -> 750.0.
  - `motor_gain_follows_the_command_and_is_silent_at_zero`: cmd 0 -> gain 0; cmd 1 -> gain > cmd 0.5's gain; every gain in [0, 1].
  - `each_motor_is_its_own_voice`: rpm `[10000, 20000, 30000, 40000]` -> four distinct ascending frequencies.
  - `wind_rises_with_airspeed`: speeds 0, 10, 30 -> strictly increasing `wind.gain` and `wind.cutoff_hz`; cutoff at 0 is 300.0 and at 30 and at 60 is 3500.0; gain at 0 is 0.
  - `wash_rises_with_descent_and_throttle`: same cmds, descent 0 vs 5 -> larger wash at 5; mean cmd 0.2 vs 0.8 -> larger at 0.8; wash 0 when all cmds are 0.
  - `inactive_or_no_motors_is_silent`: `active: false` -> all gains 0 (motors, wash, wind); empty motors with `active: true` -> `motors` empty, wash 0.
  - `non_finite_input_is_silent_and_params_are_finite`: NaN/inf in rpm, cmd, airspeed, descent -> every field of the result finite and gains 0 for the bad part (a NaN rpm gives that motor gain 0 and frequency 0); negative rpm/cmd/airspeed clamp to 0.
  - `mismatched_lengths_use_the_shorter`: 4 rpm, 2 cmd -> 2 motors; no panic.
  - `thud_gain_maps_speed`: 0 -> 0.0; non-finite -> 0.0; 12 -> 1.0; 100 -> 1.0; 3 -> a value in (0.15, 0.9) strictly between the 1 m/s and 12 m/s values; monotone non-decreasing; any speed > 0 gives a gain >= 0.1 (the floor so a scrape is audible).
  And `tests/isolation.rs`: `no_simulation_crate_depends_on_audio`: for every `../ofs-*/Cargo.toml` except `ofs-audio`, `ofs-client`, `ofs-godot` (client side), the text does not contain `ofs-audio`; and `ofs-audio`'s own `Cargo.toml` has an empty `[dependencies]` section (no lines between `[dependencies]` and the next header or EOF other than blank/comments).

- [ ] **Step 2: Run to verify failure.** `cargo test -p ofs-audio` (in WSL) -> compile error, no crate.

- [ ] **Step 3: Implement** the crate per the Interfaces block. `Cargo.toml` uses `version.workspace = true` etc. like `ofs-core`; description "Procedural flight audio: the pure sound parameters and a deterministic synth (no Godot, no I/O)". `params`: per motor `i < min(len(rpm), len(cmd))`: `freq = rpm.max(0) / 60 * blades` and `gain = clamp(cmd,0,1)^1.5 * motor_gain / sqrt(n)`; a non-finite rpm or cmd zeroes that motor's freq and gain. `wash = mean² * (0.25 + 0.75 * clamp(descent / ref, 0, 1)) + 0.5 * spread * mean` (mean and spread over the finite cmds; spread = max - min), clamped to [0, 1]. `wind.gain = clamp((airspeed / wind_ref)^exp, 0, 1)`; `wind.cutoff_hz` linear from min to max over `0..=wind_cutoff_full_mps`, clamped. Non-finite airspeed/descent -> 0. `active == false` returns the same shape with every gain 0 (keep `motors` length, freq as computed or 0). `thud_gain`: `0` for non-finite or `<= 0`; else `clamp(speed/ref, 0, 1)^exp` raised to at least 0.1.

- [ ] **Step 4: Run to verify pass.** `cargo test -p ofs-audio` -> all pass; `cargo clippy -p ofs-audio --all-targets -- -D warnings` clean.

- [ ] **Step 5: Commit** `feat(audio): the pure sound parameters (#158)`.

---

### Task 2: `Synth` and the `Audio` facade

**Files:**
- Create: `crates/ofs-audio/src/synth.rs`, `crates/ofs-audio/src/audio.rs`, `crates/ofs-audio/tests/synth.rs`, `crates/ofs-audio/tests/audio.rs`
- Modify: `crates/ofs-audio/src/lib.rs` (add `mod synth; mod audio;` and re-export `Synth`, `Audio`)

**Interfaces:**
- Consumes: Task 1's `AudioConfig`, `AudioInput`, `AudioParams`, `params`, `thud_gain`, `SAMPLE_RATE`.
- Produces:
  - `pub struct Synth` with `pub fn new(sample_rate: u32) -> Synth`, `pub fn set_params(&mut self, p: &AudioParams)`, `pub fn trigger_thud(&mut self, gain: f64)`, `pub fn render(&mut self, out: &mut [f32])` (mono, samples in [-1, 1]), `pub fn reset(&mut self)` (silence and restart the noise seed: a fresh synth).
  - `pub struct Audio` with `pub fn new() -> Audio` (config default, `Synth::new(SAMPLE_RATE)`), `pub fn collision_event(&mut self, time_s: f64)`, `pub fn frame(&mut self, input: &AudioInput, state_time_s: f64, collision_speed_mps: f64, frames: usize) -> Vec<f32>`, `pub fn reset(&mut self)`.

- [ ] **Step 1: Write the failing tests.** `tests/synth.rs` (helper `goertzel(samples, freq, rate) -> f64` magnitude):
  - `same_calls_give_identical_samples`: two fresh synths fed the same params, a thud and the same buffer sizes produce `==` sample vectors (compare bit patterns via `to_bits`).
  - `a_motor_tone_peaks_at_its_blade_pass`: one motor at 1500 Hz gain 0.5, render 0.25 s after a 0.1 s warm-up; Goertzel at 1500 > 5x Goertzel at 1000 and at 2200.
  - `the_tone_follows_a_changed_frequency`: set 1000 Hz, render, set 2000 Hz, render 0.2 s, discard the first 0.1 s of it, Goertzel at 2000 > 5x at 1000.
  - `silence_is_exact_after_the_slew`: default params (all zero), render 0.1 s then 0.05 s -> all samples `== 0.0` in the last block.
  - `wind_is_quieter_low_passed_at_low_cutoff`: noise-only params with cutoff 300 vs 3500 at the same gain: the RMS of the 3500 render is larger than of the 300 render.
  - `thud_is_audible_and_decays`: RMS of the first 50 ms after `trigger_thud(1.0)` > 0.02 and > 10x the RMS of the 400-500 ms window; `trigger_thud(0.0)` stays silent.
  - `output_stays_in_range_and_finite`: four motors at gain 1.0, rpm 1e9, wind gain 1, wash 1, plus a thud: every sample finite and `|s| <= 1.0`; also with NaN in the params (frequency, gains, cutoff): finite samples (a NaN param is treated as 0).
  - `a_zero_length_buffer_is_fine`: `render(&mut [])` does not panic.
  `tests/audio.rs`:
  - `a_collision_waits_for_the_state_that_carries_it`: `collision_event(5.0)`; `frame(.., state_time 4.9, speed 0.0, ..)` renders silence (no thud: max abs < 1e-6 after the warm-up); then `frame(.., state_time 5.0, speed 8.0, ..)` has a thud (RMS of the next 50 ms > 0.02); a further frame at time 5.1 does not thud again.
  - `reset_drops_a_pending_collision`: `collision_event(1.0)`, `reset()`, `frame(.., 2.0, 8.0, ..)` has no thud.
  - `frame_is_deterministic`: two `Audio` fed the same sequence of `(input, time, speed, frames)` return equal `Vec<f32>`s.
  - `inactive_input_fades_to_silence`: an active spinning input for 0.3 s, then `active: false` for 0.2 s: the last 50 ms are all zero.

- [ ] **Step 2: Run to verify failure** (`cargo test -p ofs-audio`; missing items).

- [ ] **Step 3: Implement `Synth`.** Per motor a phase accumulator (`f64`, wrapped) with the tone `sin(p) + 0.35 sin(2p) + 0.15 sin(3p)` at the blade-pass frequency, plus `0.3 sin(p/blades)`-free simplicity: the spec's weaker fundamental is optional here, use the three harmonics only and note it as the call. Per-sample one-pole slew (time constant ~20 ms) of every motor's frequency and gain, the wind gain/cutoff and the wash gain, toward `set_params` targets; any non-finite target is replaced by 0; the number of voices follows the latest `set_params` (new voices start at gain 0). Noise from a fixed-seed xorshift64 (one generator, deterministic, consumed in a fixed order). Wind = noise through a one-pole low-pass at the slewed cutoff times gain; wash = noise through a one-pole high-pass-ish (noise minus its own low-pass at ~600 Hz) times gain. Thud: when triggered (`gain` clamped to [0, 1], non-finite -> ignored) restart an envelope `exp(-t/0.04)` (about 120 ms to inaudible) driving `sin(2*pi*90*t)` plus a 5 ms noise click through a low-pass; thud and noise share the mix. The mix is the sum divided to keep headroom, then `tanh`-soft-clipped (the clip guarantees `|s| <= 1`). `SAMPLE_RATE` frames per second drive all time constants.

- [ ] **Step 4: Implement `Audio`.** Holds `AudioConfig`, `Synth`, `pending: Vec<f64>` (event times). `collision_event` pushes the time (ignore non-finite). `frame`: remove queued times `<= state_time_s` (a non-finite `state_time_s` removes none); if any was removed and `collision_speed_mps > 0`, `trigger_thud(thud_gain(speed))` once; then `set_params(&params(input, &cfg))`, render `frames` samples. `reset` clears `pending` and resets the synth. Cap `pending` at 16 entries (drop the oldest) so a stalled state stream cannot grow it.

- [ ] **Step 5: Run to verify pass** (`cargo test -p ofs-audio`, clippy `-D warnings`).

- [ ] **Step 6: Commit** `feat(audio): the deterministic synth and the Audio facade (#158)`.

---

### Task 3: `motor_rpm` in the telemetry and the Godot binding

**Files:**
- Modify: `crates/ofs-client/src/model.rs` (`Telemetry`), `crates/ofs-client/tests/model.rs`, `crates/ofs-godot/Cargo.toml`, `crates/ofs-godot/src/lib.rs`, `godot/tests/test_extension.gd`

**Interfaces:**
- Consumes: Task 2's `ofs_audio::{Audio, AudioInput, SAMPLE_RATE}`.
- Produces: `Telemetry.motor_rpm: Vec<f64>` (field after `motor_cmd`); GDScript API on `OfsClient`: `audio_sample_rate() -> float`, `audio_render(frames: int, active: bool) -> PackedVector2Array`; `get_telemetry()` gains `motor_rpm` (PackedFloat32Array).

- [ ] **Step 1: Failing tests.** In `crates/ofs-client/tests/model.rs` (read it for the existing style) add `telemetry_carries_the_motor_rpm`: a `pb::State` with `motor_rpm: vec![100.0, 200.0]` gives `Telemetry::from_pb(..).motor_rpm == vec![100.0, 200.0]`. In `godot/tests/test_extension.gd` (read it; it uses the real extension) add checks: `audio_sample_rate()` is 44100.0; with no session `audio_render(441, true).size() == 441` and every sample is `Vector2.ZERO`; `audio_render(0, true).size() == 0`.

- [ ] **Step 2: Run to verify failure** (client test: compile error; Godot test: missing method).

- [ ] **Step 3: Implement.** Add `motor_rpm: s.motor_rpm.clone()` to `Telemetry::from_pb`. `ofs-godot` depends on `ofs-audio` (workspace dep). `OfsClient` gains `audio: ofs_audio::Audio` (created in `init`). In `process`, `Update::Event` of kind `Collision` also calls `self.audio.collision_event(event.time_s)`; `Update::Phase` with phase `Loading` calls `self.audio.reset()`. `audio_render(frames, active)`: with no client or no telemetry, or `!active`, build `AudioInput { active: false, ..Default }` and `state_time_s = f64::NAN`, speed 0; else from `Telemetry`: `motor_rpm`, `motor_cmd`, `airspeed_mps = speed_mps` (ground speed: no wind model yet, comment says so), `descent_mps = -climb_mps`, `active: true`; call `self.audio.frame(&input, t.time_s, t.collision_speed_mps, frames.max(0) as usize)` and return the mono samples as a `PackedVector2Array` of `(s, s)`. Document both `#[func]`s and the new telemetry key in their doc comments (the long `get_telemetry` doc lists keys).

- [ ] **Step 4: Run to verify pass.** `cargo test -p ofs-client` in WSL, `cargo build -p ofs-sim -p ofs-godot`, then the Godot unit suite (`bash scripts/run-godot-tests.sh unit`); read the whole output.

- [ ] **Step 5: Commit** `feat(audio): motor rpm in the telemetry and OfsClient.audio_render (#158)`.

---

### Task 4: Live playback and the "Sound" setting in Godot

**Files:**
- Create: `godot/drone/flight_audio.gd`, `godot/tests/test_flight_audio.gd`
- Modify: `godot/scripts/settings.gd`, `godot/ui/settings_screen.gd`, `godot/scripts/app.gd`, `godot/main.tscn` (only if the node is added there; prefer creating it in `app.gd`'s `_ready`), `godot/tests/run_tests.gd` (register the suite), `godot/tests/test_settings.gd`, `godot/tests/test_settings_screen.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: `OfsClient.audio_sample_rate()`, `OfsClient.audio_render(frames, active)`.
- Produces: setting key `audio` (bool, default `true`; env `OFS_AUDIO`, flag `--audio`); settings row `{"key": "audio", "label": "Sound", "kind": "bool", "live": true}`; `flight_audio.gd` (`extends Node`): `const BUS := "Flight"`, `func setup(client: Object) -> void` (creates the `Flight` bus under Master if missing, an `AudioStreamPlayer` on it with an `AudioStreamGenerator` at `client.audio_sample_rate()` and `buffer_length = 0.1`, and starts playing), `var enabled := true`, `var active := false`, `func set_enabled(on: bool) -> void` (sets `enabled` and `AudioServer.set_bus_mute`), `func pump() -> int` (pushes frames and returns how many were pushed: `0` when `enabled` is false), called from `app.gd._process`.

- [ ] **Step 1: Failing tests.** `test_settings.gd`: `audio` defaults true; `resolve({"audio": false}, {}, [])` gives false; env `OFS_AUDIO=off` and flag `--audio=false` give false (follow the file's existing style for `video_effects`); `overrides` reports `audio` for the flag. `test_settings_screen.gd`: the screen has an `audio` control (CheckBox) showing the setting, and toggling it emits `["audio", false]`; the row is live (empty note). `test_flight_audio.gd` (use `add_to_tree`, a fake client object with `audio_sample_rate()` and `audio_render(frames, active)` recording its calls and returning `PackedVector2Array` of that size): after `setup`, `pump()` with `enabled` returns > 0 and the fake saw `active == node.active`; with `set_enabled(false)`, `pump()` returns 0 and the fake is not called; `set_enabled(false)` mutes the `Flight` bus and `set_enabled(true)` unmutes it (`AudioServer.is_bus_mute`); `setup` twice does not add a second `Flight` bus; the pushed frames never exceed `get_frames_available()` (assert the fake's requested frames <= generator capacity `0.1 * rate`). `e2e_open_loop.gd` (read it): one check that after the flight starts the node exists and `active` becomes true while flying (the phase is "flying").

- [ ] **Step 2: Run to verify failure** (`bash scripts/run-godot-tests.sh unit`).

- [ ] **Step 3: Implement.** `settings.gd`: `"audio": true` in `defaults()`, `"OFS_AUDIO": "audio"` and `"--audio": "audio"` in the two tables. `settings_screen.gd`: the row, placed after "Fullscreen". `flight_audio.gd` as in Interfaces; `pump()` asks `client.audio_render(mini(generator_playback.get_frames_available(), int(0.1 * rate)), active)` only when `enabled` and the available count is > 0 and pushes the result with `push_buffer`. `app.gd`: a `FlightAudio` child created in `_ready` after the client (`flight_audio.setup(client)`, `set_enabled(settings["audio"])`), `_process` sets `flight_audio.active = client.get_phase() == "flying"` and calls `pump()`, and `_on_setting_changed` gets an `"audio"` case calling `set_enabled(value)`. Switching sound on must not play a stale burst: `set_enabled(true)` clears the generator (`player.stop()` / `play()` resets the playback) so the buffer restarts empty.

- [ ] **Step 4: Run to verify pass.** `bash scripts/run-godot-tests.sh all` in WSL (SITL e2e only if `OFS_SITL_LAUNCH` is set); read the whole output: every suite `0 failed`, no `SCRIPT ERROR`.

- [ ] **Step 5: Commit** `feat(godot): live flight audio and the Sound setting (#158)`.

---

### Task 5: Documentation and the CHANGELOG

**Files:**
- Modify: `CHANGELOG.md` (an entry at the top of Unreleased: Added, one paragraph on the setting and the new crate), `README.md` (the settings list near line 120: `audio`/`OFS_AUDIO`/`--audio`, and one sentence that the sim has sound), `docs/architecture.md` (the crate table row for `ofs-audio` and a short "Sound" section: derived on the client from state, outside the digest, the bus layout, how #159 reuses it), `docs/dev-setup.md` (a short "Flight audio" note in the Godot section: headless has no audio device, the manual check list from the spec's Done-when)

**Interfaces:** Consumes everything above; produces nothing for code.

- [ ] **Step 1:** Write the four edits; the CHANGELOG entry states "no protocol change (still 15)".
- [ ] **Step 2:** `grep -n "ofs-audio" docs/architecture.md CHANGELOG.md` shows the row and the entry; LF check on all touched files.
- [ ] **Step 3: Commit** `docs(audio): the audio design, the setting and the changelog (#158)`.
