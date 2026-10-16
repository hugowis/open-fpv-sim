# M6c — Validation harness, repro bundle and phase close Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close M6 and phase 2: a repro bundle that a failed session writes and a script replays deterministically (#40), a documented data-submission format with a thrust-stand comparator and one worked report (#49), and the phase-close docs.

**Architecture:** Two independent halves. *Repro bundle:* the `Vehicle` keeps a **journal** of everything that steers a run (stick changes, transmitter, faults, played inputs), stamped by tick; a rolling in-memory **tail recorder** keeps the last 10 s of the default channel table; when a session fails, `Session` writes a self-contained folder (quad, library, world, seed, journal, firmware state, `tail.mcap`); `ofs-sim --replay <folder>` rebuilds the vehicle, replays the journal tick for tick and says whether the same failure came back; `ofs.repro` wraps that for scripts. *Validation:* a new crate `ofs-validate` defines the submission folder (`submission.toml` + CSV), validates it with every problem collected, and for thrust-stand submissions drives the real `EscMotor` + `Propeller` models to steady state per measured row and reports the error; one synthetic example ships with its generated report, and CI prints the report in the job summary (tracked, not gating).

**Tech Stack:** Rust workspace (`ofs-log`, `ofs-sim`, `ofs-config`, new `ofs-validate`), Python client (`ofs.repro`), TOML/CSV data, Markdown docs.

**Spec:** There is no dedicated M6 spec. This plan argues from the M6 section of `docs/superpowers/specs/2026-10-13-phase2-real-gear-trusted-numbers-design.md` §4 (exit criteria: "the validation harness runs in CI tracked with one worked report; the repro bundle fires on a real error"), the parent spec §7 (repro bundle) and §8.5 (validation reports), and the "done when" lines of issues #40 and #49. The calls those leave open are in "Design calls"; the first four were put to the maintainer and answered, the rest are the plan author's, for review. M6a and M6b are merged (PRs #78, #79).

## Design calls

Answered by the maintainer:
1. **The bundle carries the last 10 s of MCAP from an always-on rolling recorder.** Every session keeps its tail in memory (a few MB); a failure always has it, with no `start_recording` needed.
2. **The bundle is a folder** (`<logs dir>/<quad stem>-<n>.repro/`), not an archive: no new dependency; zip it to attach it to an issue. `ofs.repro.replay_bundle` (Python) and `ofs-sim --replay` (Rust) replay a folder.
3. **Thrust-stand comparator ships; Blackbox is a defined, schema-checked kind only.** Its step-response comparator needs SITL and a real tune, and waits for real data.
4. **The worked report uses a synthetic submission labelled `estimated`:** hand-written datasheet-style figures that demonstrate the format and the report, said plainly not to be a measurement.

The author's:
5. **Journal, not input recording, is what replays.** Entries (tick-stamped): `sticks` (only when a value changes), `tx`, `fault on|off`, `schedule` (a fault with its `at_s`), `clear`, `play <csv>`, `stop`. Applied before the tick they are stamped with, through the same `Vehicle` methods the server calls, so a replay is the same call sequence. Plain text, one entry per line, floats in Rust's shortest round-trip form (exact on re-read); a played inputs file is written beside it as `inputs-<n>.csv` in the `play_inputs` format. At most 250 000 entries (about an hour of 50 Hz stick changes); past that the journal stops growing, the bundle says `journal_truncated = true` and the replay reports `exact = false`.
6. **Exactness is honest.** `exact = true` only for a lockstep session with an intact journal. A real-time session (SITL runs on its own clock) still writes a bundle and replays in lockstep, but `exact = false` and the replay says "approximate".
7. **A failure is reproduced when** the replay fails with the same error text at the same tick *and* the bus digest then equals the recorded one. Anything else is "not reproduced" with both sides printed (a SITL crash that does not recur is the expected case).
8. **A bundle is written once per failed session**, for firmware failures, non-finite values and panics (everything `Session` poisons on). Writing it must never mask the error: if it fails, the original `SimError` and its event are unchanged and the bundle error is logged and appended to the event text. A failed `Load` writes nothing (config errors are the user's).
9. **No protocol change.** The bundle path travels in the `SimError` event text (`... (repro bundle: <path>)`); scripts find bundles with `ofs.repro.find_bundles(sim.logs_dir)`.
10. **Bundle contents:** `bundle.toml` (format_version 1, reason, failed tick/time, seed, mode, base_hz, fc, quad/world file names, window, digest, exactness), `quads/<quad>.toml` + its Betaflight diff, `library/` (the whole library root the loader used, `OFS_LIBRARY` honoured), `worlds/<world>.toml`, `journal.txt` (+ `inputs-<n>.csv`), `tail.mcap`, and for Betaflight sessions `firmware/` (`eeprom.bin` and `betaflight.diff` as they were right after `Load`, plus the final `sitl.log` tail). The replay loads the bundle's own `quads/` + `library/` with the library env override ignored, so it never picks up the replayer's files.
11. **Submission format** (`docs/validation.md`): a folder with `submission.toml` (`schema_version = 1`, `kind`, `title`, `quad` path relative to the file, `[origin]` with `source` ∈ `datasheet|measured|estimated`, `note`, `contributor`, `license = "CC-BY-SA-4.0"`, `data` file name) and the data file. `thrust_stand`: CSV with columns `command` (ESC command 0..1), `voltage_v`, and any of `rpm`, `thrust_n`, `current_a` (battery-side current; a blank cell is "not measured"), plus `motor = <index>` in `submission.toml`. `blackbox_step`: the decoded CSV from `blackbox_decode` (header names `time (us)`, `gyroADC[0..2]`, `setpoint[0..2]`, from blackbox-tools as remembered — the validator reports the header it finds, and these names are the first thing to check against a real file), the quad's Betaflight diff (`diff = "..."`).
12. **Thrust-stand comparison.** For each row the sim's own `EscMotor` + `Propeller` (parameters from the quad via one shared `vehicle::drivetrain_params`, so it cannot drift from `build`) run at the row's `voltage_v` and `command` until `motor.0.omega_radps` changes by less than 1e-6 relative per simulated second (cap 10 s, 8 kHz); the sim's current is the battery-side `esc.0.bus_current_a`. Standard sea-level air as in the model; no density correction. Error is `(sim − measured)/measured`, rows with a measured value of 0 are skipped for that quantity. The report prints per-row values and, per quantity, mean absolute and maximum error in percent. **Tracked, not gating:** the tool exits 0 whenever the submission is valid, whatever the error; `--max-error-pct` is an opt-in gate nobody uses yet.
13. **Synthetic baseline rows** (`command, voltage_v, rpm, thrust_n, current_a`): `0.20, 24.0, 14000, 2.2, 3.0` · `0.30, 23.9, 18000, 3.6, —` · `0.40, 23.8, 21500, 5.1, 11.0` · `0.60, 23.4, 27000, 8.0, 24.0` · `0.80, 22.9, 31000, 10.6, 38.0` · `1.00, 22.4, 34000, 12.7, 52.0`. They are a plausible shape for a 2207 1950 KV motor with a 5-inch prop on 6S, not data. The shipped model (flat `Ct 0.11`, `Cp 0.045`) is expected to disagree (it spins faster and draws less); the report showing that is the point of the example.

## Global Constraints

- Every task leaves `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passing; MSRV is Rust 1.85 (`cargo +1.85 check --workspace --all-targets --locked --exclude ofs-godot` at the end). `cargo fmt` is not a gate; do not run it on existing files.
- The journal and the tail recorder only *read* the bus: **the radio goldens (`crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`) must not change**, and `cargo test -p ofs-sim --test radio_golden --locked` stays green on both OSes (Linux from WSL: `CARGO_TARGET_DIR=$HOME/ofs/target ~/.cargo/bin/cargo test ...` from `/mnt/c/...`). A changed digest is a bug in this plan's code, not a golden to regenerate.
- Determinism: no RNG in new code; two replays of one bundle give identical output; bundle writing sorts directory listings.
- Recording and replay of floats is exact: write with `{}`, read with `str::parse::<f64>()`; never `{:.N}` in the journal or `bundle.toml`.
- Real-time overhead: the open-loop real-time factor (`python scripts/benchmark.py --binary target/release/ofs-sim --floor 1`) is measured on `main` before Task 2 and after Task 3 and both numbers go in the ledger; a drop of more than 10 % is reported to the maintainer instead of accepted silently.
- Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Work on a branch in a worktree. Do not tick items in the epic issue #73.
- `ofs-fc` `sitl_errors::a_victim_that_ignores_sigterm_is_sigkilled` fails on untouched `main` under WSL: report it, do not chase it.

## Review Focus

Failure modes a straight reading of the tasks would not test; each has its test in the named task.

1. A session that fails twice (a poison after a poison) or fails after its bundle exists writes exactly one bundle; a re-`Load` starts clean (Task 3).
2. The logs directory is read-only, missing or full: the `SimError` and its event still arrive, with the bundle error appended; no panic (Task 3).
3. A journal past the cap, a real-time session, and a bundle whose `inputs-<n>.csv` was deleted: replay says `exact = false` / refuses with the file name, and never claims "reproduced" it cannot back (Tasks 1, 3).
4. A quad outside the repo, a path with spaces, a world given by name, `OFS_LIBRARY` set at write time and different at replay time (Task 3).
5. The tail window around the boundary: fewer than 10 s recorded, exactly 10 s, a long run; the snapshot is a valid MCAP in every case and its first record is no older than the window (Task 2).
6. Hot path: the tail recorder reuses evicted buffers (no allocation per message in steady state) and the benchmark floor holds (Task 2).
7. Submission folders with a BOM or CRLF in the CSV, an unknown column, a blank cell, `command` outside 0..1, `voltage_v <= 0`, a `motor` index the quad lacks, a missing `license`, a wrong `schema_version`: all problems reported together with file and line; none panics (Tasks 4, 5).
8. A thrust-stand row the model cannot reach (a command that saturates the ESC current limit) still converges or reports "no steady state" for that row instead of looping; rows with a measured 0 do not divide by zero (Task 5).
9. The committed `REPORT.md` equals what the tool prints now, so the example cannot go stale (Task 5).

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `crates/ofs-sim/src/journal.rs` | create | `Action`, `Journal`, text format read/write |
| `crates/ofs-sim/src/vehicle.rs` | modify | journal hooks in the public steering methods; `journal()`, `replay_action`, `snapshot_tail`; `drivetrain_params` |
| `crates/ofs-log/src/tail.rs` | create | `TailHandle`, `TailRecorder` (rolling in-memory recorder) |
| `crates/ofs-sim/src/repro.rs` | create | `BundleSource`, `write_bundle`, `replay`, `snapshot_firmware` |
| `crates/ofs-sim/src/session.rs`, `server.rs`, `runner.rs` | modify | capture the bundle on failure; event text |
| `crates/ofs-sim/src/main.rs` | modify | `--replay <bundle>` |
| `crates/ofs-config/src/lib.rs` | modify | `load_with_library_env` (replay ignores `OFS_LIBRARY`) |
| `python/ofs/repro.py`, `python/ofs/__init__.py` | create/modify | `find_bundles`, `replay_bundle`, `ReplayResult` |
| `crates/ofs-validate/` | create | submission schema, validation, thrust-stand comparator, report, binary |
| `validation/thrust-stand/example-2207-5in/` | create | the synthetic submission and its generated `REPORT.md` |
| `.github/workflows/ci.yml` | modify | run the example report in `core`, into the job summary |
| `docs/repro-bundle.md`, `docs/validation.md` | create | the two formats, for users and contributors |
| `docs/roadmap.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `docs/architecture.md`, parent spec §9 | modify | phase close (Task 7) |

---

### Task 1: The session journal (#40)

**Files:** create `crates/ofs-sim/src/journal.rs` (+ `pub mod journal;` in `lib.rs`); modify `crates/ofs-sim/src/vehicle.rs`; test in `journal.rs` unit tests and `crates/ofs-sim/tests/journal.rs`.

**Interfaces:**
- Produces:
  - `pub const MAX_ENTRIES: usize = 250_000;`
  - `pub enum Action { Sticks([f64; 8]) /* roll, pitch, yaw, throttle, aux1..4 */, Transmitter(bool), Fault { fault: Fault, active: bool }, ScheduleFault { fault: Fault, at_s: f64 }, ClearFaults, PlayInputs(Inputs), StopInputs }` (`Debug, Clone, PartialEq`)
  - `pub struct Entry { pub tick: u64, pub action: Action }`
  - `pub struct Journal` with `new()`, `push(&mut self, tick: u64, action: Action)` (past `MAX_ENTRIES`: drops it and sets the flag), `entries(&self) -> &[Entry]`, `truncated(&self) -> bool`, `write(&self, dir: &Path) -> std::io::Result<()>` (`journal.txt` + `inputs-<n>.csv`), `read(dir: &Path) -> Result<Journal, String>` (every problem reported with file and line).
  - `Vehicle::journal(&self) -> &Journal`; `Vehicle::replay_action(&mut self, action: &Action) -> Result<(), SimError>`.
- Text format of `journal.txt`: first line `# ofs-journal 1`, optional `# truncated`, then `<tick> <verb> <args>`: `sticks v1..v8` · `tx 0|1` · `fault on|off <name> key=value...` · `schedule <at_s> <name> key=value...` · `clear` · `play inputs-<n>.csv` · `stop`. Fault keys are the variant's field names (`bias_radps` and `position_ned_m` as `x,y,z`; `RadioLinkLoss` has none).

- [ ] **Step 1: Write the failing tests.** In `journal.rs`: `every_fault_survives_a_text_round_trip` (all 8 `Fault` variants, with awkward floats `0.1`, `1e-300`, `-0.0`, `f64::MAX`), `sticks_with_nan_and_infinity_survive` (`NaN`, `inf`, `-inf` re-read as the same bits for `inf`; `NaN` is NaN), `a_played_inputs_file_is_written_beside_the_journal_and_read_back_equal` (rows compare equal after `inputs::parse`), `a_journal_past_the_cap_is_truncated_and_says_so` (push `MAX_ENTRIES + 1`; `truncated()`, `len == MAX_ENTRIES`; the `# truncated` line survives a round trip), `read_reports_every_bad_line_with_its_number` (an unknown verb, a missing field, a non-number, in one message). In `tests/journal.rs`: `a_vehicle_journals_what_steers_it` (open-loop quad: `set_sticks` twice with the same values then different → 2 entries, not 3; `set_transmitter(false)`; `set_fault`; `schedule_fault(..., at)`; `clear_faults`; `play_inputs`; `stop_inputs` — the entries in order with the tick the call was made at; a scheduled fault that later *activates* adds no entry; the initial `build` calls add none), `replaying_a_journal_reproduces_the_run` (run A: a script of steering calls and `step_ticks` between them; run B: fresh vehicle, same seed, apply `A.journal()` entries with `replay_action` stepping to each entry's tick; `A.digest() == B.digest()` and `A.state() == B.state()` at the end).
- [ ] **Step 2: Run them.** `cargo test -p ofs-sim journal --locked`. Expected: FAIL to compile (`journal` missing).
- [ ] **Step 3: Implement.** `Fault ↔ tokens` and the reader/writer in `journal.rs`. In `vehicle.rs`: a `journal: Journal` field; `set_fault`, `schedule_fault`, `clear_faults`, `play_inputs`, `stop_inputs`, `set_sticks`, `set_transmitter` journal on the way in (`set_sticks` only when the eight values differ from the last journaled ones; `schedule_fault` and `play_inputs` only when they return `Ok`). Move the body of `set_fault` into a private `apply_fault` that `schedule_fault` (at-now case) and `apply_due` call, so internal activation never journals. End of `build`: `vehicle.journal = Journal::new()` after the initial `set_sticks` / `set_transmitter` / identities. `replay_action` calls the public methods (so replaying journals again, harmlessly).
- [ ] **Step 4: Run.** `cargo test -p ofs-sim --locked` then `cargo clippy --workspace --all-targets --locked -- -D warnings`. Expected: PASS, no warnings; `radio_golden` unchanged.
- [ ] **Step 5: Commit** `feat(sim): a tick-stamped journal of everything that steers a session (#40)`.

---

### Task 2: The rolling tail recorder (#40)

**Files:** create `crates/ofs-log/src/tail.rs` (+ `pub mod tail;`); modify `crates/ofs-log/src/recorder.rs` (extract the per-signal JSON encode so both recorders share it), `crates/ofs-sim/src/vehicle.rs`; tests `crates/ofs-log/tests/tail.rs`, `crates/ofs-sim/tests/tail.rs`.

**Interfaces:**
- Consumes: `spec::resolve`, `McapSink`, the shared encode helper.
- Produces: `pub const DEFAULT_WINDOW_S: f64 = 10.0;` · `#[derive(Clone, Default)] pub struct TailHandle` with `snapshot(&self, path: &Path) -> Result<RecordingSummary, RecordError>` (writes the retained messages to a new MCAP, refuses an existing file) · `pub struct TailRecorder` with `new(handle: TailHandle, bus: &Bus, base_hz: u32, window_s: f64) -> Result<Self, RecordError>` (records the default table over every signal the bus has now), implementing `Model` (rate divisor 1, reads only) · `Vehicle::snapshot_tail(&self, path: &Path) -> Result<RecordingSummary, RecordError>`.
- Storage: a `VecDeque` of `(channel, time_ns, bytes)`; evicting messages older than `window_s` returns their `Vec<u8>` to a free list that the next record reuses.

- [ ] **Step 1: Write the failing tests.** `tests/tail.rs` (ofs-log): `the_snapshot_holds_only_the_last_window` (a scheduler with a ramp signal, window 2 s, run 7 s: `first_s >= 5.0 - 1/rate`, `last_s == 7.0`, readable with the `mcap` reader), `fewer_seconds_than_the_window_keeps_everything` (run 0.5 s with window 2 s: `first_s` is the first record), `the_snapshot_matches_the_plain_recorder_over_the_same_span` (run a `Recorder` and a `TailRecorder` on one bus; the tail's messages equal the plain recording's messages from `first_s` on, channel by channel), `two_identical_runs_give_identical_snapshot_bytes`, `a_snapshot_refuses_an_existing_file`, `steady_state_recording_reuses_buffers` (count free-list hits: after the first window, `allocations_since_warmup == 0`; expose the counter `#[doc(hidden)]`). `ofs-sim/tests/tail.rs`: `a_built_vehicle_has_a_tail_of_its_last_ten_seconds` (open-loop quad, run 12 s, snapshot: `first_s >= 2.0 - 0.01`, includes `body.pos_ned_m` and `battery.voltage_v`).
- [ ] **Step 2: Run them.** `cargo test -p ofs-log -p ofs-sim tail --locked`. Expected: FAIL (module missing).
- [ ] **Step 3: Implement.** `tail.rs` as above; in `build`, after `Handles::register` and before the scheduler takes the bus, create the `TailRecorder` and add it last (after `Recorder`); the vehicle keeps the `TailHandle`. Resolution failure cannot happen for the default table over the bus's own signals; map it to `SimError::Other` anyway.
- [ ] **Step 4: Measure and run.** `cargo test --workspace --locked`; `cargo build -p ofs-sim --release --locked` on `main` (before) and on the branch, `python scripts/benchmark.py --binary target/release/ofs-sim --floor 1` each; ledger both numbers. Goldens unchanged.
- [ ] **Step 5: Commit** `feat(log): a rolling in-memory tail recorder, always on (#40)`.

---

### Task 3: The bundle writer, `--replay` and the failure hook (#40)

**Files:** create `crates/ofs-sim/src/repro.rs` (+ `pub mod repro;`); modify `crates/ofs-config/src/lib.rs`, `crates/ofs-sim/src/{session,server,runner,main}.rs`, `Cargo.toml` of `ofs-sim` (`toml.workspace = true`); tests in `repro.rs`, `session.rs` and `crates/ofs-sim/tests/repro.rs`; one live case in `crates/ofs-sim/tests/sitl_live.rs`.

**Interfaces:**
- Consumes: `Journal`, `Vehicle::{journal, snapshot_tail, replay_action, digest}`, `firmware_dir`.
- Produces in `ofs-config`: `pub fn load_with_library_env(path: &Path, library_env: Option<&str>) -> Result<QuadConfig, ConfigError>`; `load` calls it with the real env.
- Produces in `repro.rs`:
  - `pub struct BundleSource { pub quad_path: PathBuf, pub world_path: Option<PathBuf>, pub seed: u64, pub open_loop_fc: bool, pub realtime: bool, pub logs_dir: PathBuf, pub firmware_dir: Option<PathBuf>, pub firmware_initial: Vec<(String, Vec<u8>)> }`
  - `pub fn snapshot_firmware(dir: &Path) -> Vec<(String, Vec<u8>)>` (`eeprom.bin`, `betaflight.diff`; empty when absent)
  - `pub fn write_bundle(src: &BundleSource, vehicle: &Vehicle, error: &SimError) -> Result<PathBuf, String>` (design call 10; the directory name from `next_numbered_path(&logs_dir, &stem, "repro")`)
  - `pub struct ReplayOutcome { pub reproduced: bool, pub exact: bool, pub reached_tick: u64, pub expected_tick: u64, pub failure: Option<String>, pub expected_failure: String, pub digest: u64, pub expected_digest: u64 }`
  - `pub fn replay(bundle: &Path, data_dir: &Path) -> Result<ReplayOutcome, String>` (loads the bundle's `quads/` with `library_env = None`; Betaflight bundles first copy `firmware/` into `firmware_dir(data_dir, <bundle quad>)`; steps entry by entry; stops at the first `Err` or at `expected_tick`).
- Produces in `session.rs`: `Session.repro: Option<BundleSource>`, `Session.bundle: Option<Result<PathBuf, String>>`, `Session::failure_text(&self, e: &SimError) -> String` (the error plus ` (repro bundle: <path>)` or ` (repro bundle failed: <why>)`). `Session::step` and `Session::poison` capture the bundle the first time a failure is set. The three `SimError` event sites (`server.rs` Run, `runner.rs` Err and panic arms) publish `failure_text`.
- `ofs-sim --replay <bundle> [--data-dir <dir>]` prints `key=value` lines (`reproduced`, `exact`, `reached_tick`, `expected_tick`, `digest`, `expected_digest`, `failure`, `expected_failure`; newlines in messages become spaces) and exits 0 reproduced / 1 not reproduced / 2 unusable bundle. Without `--data-dir` it uses a temporary directory it removes afterwards.

- [ ] **Step 1: Find a failing input.** Scratch test (not committed) on an open-loop vehicle at full throttle: try, in order, `Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 1e305 }` (finite, accepted; `imbalance·ω²` overflows once the rotor spins), then `PropDamage { thrust_scale: 1e305, .. }`, then `GyroFault { bias_radps: [1e308; 3], .. }`; run 2 s. Record in the ledger which yields `SimError::NonFinite`; use it as `FAILING_FAULT` below. If none does, rule: build the failure test on a hand-made `Session::poison(SimError::Firmware(..))` plus a journal-only replay check, and say so in the ledger.
- [ ] **Step 2: Write the failing tests.** `repro.rs` unit tests: `a_bundle_holds_everything_the_design_lists` (poison an open-loop session with a `BundleSource` over a temp logs dir: the folder is `<stem>-1.repro`, contains `bundle.toml`, `quads/<quad>.toml`, the diff, `library/` with the 7 part kinds, `journal.txt`, `tail.mcap` readable by the `mcap` reader, no `firmware/` for open loop), `a_second_bundle_gets_the_next_number`, `a_world_given_by_name_is_copied_as_worlds_file`, `ofs_library_at_write_time_is_followed_and_ignored_at_replay` (write with `library_env` pointing at a modified copy; replay with the env unset gives the same run), `a_path_with_spaces_works`. `session.rs` tests: `a_failed_step_writes_one_bundle` (a second `step` and a `poison` after it write none; `Session::failure_text` ends with the bundle path), `an_unwritable_logs_dir_does_not_hide_the_error` (logs path is an existing *file*: `step` still returns the original error, `failure_text` says `repro bundle failed:`), `a_panic_poison_writes_a_bundle`. `tests/repro.rs`: `replaying_a_failed_run_reproduces_it` (drive `FAILING_FAULT` through the gRPC service in lockstep with some stick changes and a scheduled second fault; take the bundle path from the `SimError` event; `replay(...)` → `reproduced`, `exact`, `reached_tick == expected_tick`, equal digests), `two_replays_agree`, `a_realtime_session_replays_inexactly` (`exact == false`), `a_truncated_journal_replays_inexactly`, `a_missing_inputs_csv_is_named_in_the_error`, `a_changed_journal_is_not_reproduced` (edit one `sticks` line in a copy: `reproduced == false`, both digests printed), and a binary test that runs `ofs-sim --replay` (`env!("CARGO_BIN_EXE_ofs-sim")`) and checks the output lines and exit codes 0, 1 and 2. Live (`#[ignore]`, runs in CI's sitl job): `a_sitl_session_failure_bundle_replays_with_its_firmware` (Betaflight quad, kill SITL mid-run via the existing helper; the bundle has `firmware/eeprom.bin`, the replay's SITL boots from it; assert the bundle is complete and the replay *runs* — a crash need not recur, so `reproduced` is not asserted).
- [ ] **Step 3: Run them.** `cargo test -p ofs-sim repro --locked`. Expected: FAIL to compile.
- [ ] **Step 4: Implement** `load_with_library_env`, `repro.rs`, the `Session` fields and hook, the event text, `main.rs --replay`, and the `Load` handler filling `Session.repro` (resolved world path, `snapshot_firmware(firmware_dir)` taken right after `build`). Library copy: recursive, sorted, regular files only.
- [ ] **Step 5: Run.** `cargo test --workspace --locked`, clippy, the WSL Linux run of `-p ofs-sim`, the benchmark again (ledger), then in WSL `cargo test -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`. Expected: PASS; goldens unchanged.
- [ ] **Step 6: Commit** `feat(sim): a failed session writes a repro bundle; ofs-sim --replay replays it (#40)`.

---

### Task 4: Python `ofs.repro`, the user doc, and the end-to-end check (#40)

**Files:** create `python/ofs/repro.py`, `python/tests/test_repro.py`, `docs/repro-bundle.md`; modify `python/ofs/__init__.py` (export `repro`), `python/README.md`, `CONTRIBUTING.md`.

**Interfaces:**
- Produces: `ofs.repro.find_bundles(logs_dir) -> list[pathlib.Path]` (sorted by number); `ofs.repro.replay_bundle(path, binary=None) -> ReplayResult` (finds `ofs-sim` like `launch`; runs `--replay`; parses the `key=value` lines; raises `ofs.OfsError` subclass `ConfigError` for exit 2 with the stderr text); `@dataclass(frozen=True) ReplayResult(reproduced: bool, exact: bool, reached_tick: int, expected_tick: int, digest: int, expected_digest: int, failure: str, expected_failure: str)`.

- [ ] **Step 1: Write the failing tests** (`test_repro.py`, no SITL): `test_a_failed_session_leaves_a_bundle_a_script_replays` (the `FAILING_FAULT` from Task 3 through the client; `NumericalError` is raised; the `sim.events()` `sim_error` text names the bundle; `find_bundles(sim.logs_dir)` returns it; `replay_bundle(...)` is `reproduced and exact`), `test_replay_of_a_missing_folder_is_a_config_error`, `test_find_bundles_ignores_other_files`.
- [ ] **Step 2: Run them.** `python -m pytest python/tests/test_repro.py -v` (with `PYTHONPATH=<worktree>/python` — the editable install points at the main checkout). Expected: FAIL (module missing).
- [ ] **Step 3: Implement** `repro.py`; write `docs/repro-bundle.md` (what a bundle holds, the exactness rule, how to attach and replay one, the 10 s window and 250 000-entry cap, the real-time caveat); add a "Reporting a crash" paragraph to `CONTRIBUTING.md` and a short section to `python/README.md`.
- [ ] **Step 4: Run.** The full Python suite and `cargo test --workspace --locked`. Expected: PASS.
- [ ] **Step 5: Commit** `feat(python): ofs.repro finds and replays repro bundles; docs (#40)`.

---

### Task 5: `ofs-validate`: the submission format and the thrust-stand comparator (#49)

**Files:** create `crates/ofs-validate/{Cargo.toml,src/lib.rs,src/submission.rs,src/thrust.rs,src/report.rs,src/main.rs}`, `crates/ofs-validate/tests/{submission.rs,thrust.rs,example.rs}`; modify `crates/ofs-sim/src/vehicle.rs` (extract `pub fn drivetrain_params(cfg: &QuadConfig, motor: usize) -> (PropParams, MotorParams, EscParams)`, used by `build`).

**Interfaces:**
- Cargo: depends on `ofs-core`, `ofs-config`, `ofs-electrical`, `ofs-physics`, `ofs-sim` (for `drivetrain_params`), `toml`, `serde`, `clap`.
- Produces:
  - `submission::load(dir: &Path) -> Result<Submission, Vec<String>>` — every problem as `"<file>[:line]: <what>"`; `Submission { kind: Kind, title, quad: PathBuf, origin: Origin, motor: usize, rows: Vec<ThrustRow> }`; `Kind::{ThrustStand, BlackboxStep}`; `Origin { source: Source, note, contributor, license }`; `Source::{Datasheet, Measured, Estimated}`; `ThrustRow { command: f64, voltage_v: f64, rpm: Option<f64>, thrust_n: Option<f64>, current_a: Option<f64> }`. A `blackbox_step` submission is validated (files exist, required columns present, `diff` exists) and carries no rows.
  - `thrust::simulate(cfg: &QuadConfig, motor: usize, row: &ThrustRow) -> Result<Predicted, String>` with `Predicted { rpm, thrust_n, current_a, settle_s }` (design call 12; `Err("no steady state within 10 s")` otherwise).
  - `report::render(sub: &Submission, rows: &[(ThrustRow, Result<Predicted, String>)]) -> String` (Markdown: title, a banner when `source = estimated`, per-row table, per-quantity mean-absolute and max error, the git-independent facts only — no dates, no paths from the machine).
  - Binary: `ofs-validate <submission dir> [--out <file>] [--summary] [--max-error-pct <n>]`; prints the report; `--summary` appends it to `$GITHUB_STEP_SUMMARY` when set; exit 0 valid (and, if `--max-error-pct` is given, within it), 1 over the gate, 2 invalid submission (problems on stderr, one per line). For `blackbox_step` the report says `not compared: the step-response comparator waits for real data (docs/roadmap.md, Validation)`.

- [ ] **Step 1: Write the failing tests.** `submission.rs`: `a_valid_thrust_stand_loads` (a fixture dir built in a tempdir), `every_problem_is_reported_with_file_and_line` (one submission with: wrong `schema_version`, unknown `kind`, missing `license`, `source = "guess"`, `motor = 9`, a CSV with an unknown column, `command = 1.5`, `voltage_v = 0`, a non-number, a row with no measured quantity — all in one `Err`), `a_bom_and_crlf_are_accepted`, `a_blank_cell_is_not_measured`, `a_blackbox_submission_is_checked_for_files_and_columns` (missing column named; the header found is printed in the message), `a_missing_data_file_is_named`. `thrust.rs`: `the_stand_matches_the_analytic_steady_state` (a hand-built quad in-memory config? use the shipped quad: at `command = 1.0`, `voltage_v = 24`, `rpm`/`thrust_n`/`current_a` satisfy `Ke·(duty·V − Ke·ω)/R − Ke·I0 = Cp·ρ·n²·D⁵/2π` within 0.1 % and `thrust = Ct·ρ·n²·D⁴`, computed in the test from the part values), `a_zero_command_gives_zero_everything`, `a_saturating_row_reports_a_prediction_not_a_loop` (current limit hit; returns `Ok` with `current_a <= esc.current_limit_a`), `settle_stops_early_for_a_quick_row` (`settle_s < 10`), `a_measured_zero_is_skipped_in_the_error` (report path). `example.rs`: `the_committed_report_is_what_the_tool_prints` (render the shipped example and compare with `REPORT.md` after normalising CRLF; numbers print at one decimal), `the_example_says_it_is_estimated`.
- [ ] **Step 2: Run them.** `cargo test -p ofs-validate --locked`. Expected: FAIL (crate missing).
- [ ] **Step 3: Implement.** Extract `drivetrain_params` first (a refactor: `cargo test -p ofs-sim --locked` and the goldens stay green), then the crate. `thrust::simulate` builds a `Bus`, one `Propeller`, one `EscMotor`, writes `battery.voltage_v` and `motor.0.cmd`, steps at 8 kHz in 0.25 s blocks until the relative change of `motor.0.omega_radps` over a block is below 1e-6·0.25, cap 10 s. Submission parsing follows the `ofs-config` style (typed `serde` structs with `deny_unknown_fields`, then a `validate` that collects).
- [ ] **Step 4: Run.** `cargo test --workspace --locked`, clippy, `cargo +1.85 check -p ofs-validate --all-targets --locked`.
- [ ] **Step 5: Commit** `feat(validate): the submission format and a thrust-stand comparator (#49)`.

---

### Task 6: The worked example, `docs/validation.md` and CI (#49)

**Files:** create `validation/README.md`, `validation/thrust-stand/example-2207-5in/{submission.toml,thrust.csv,REPORT.md}`, `docs/validation.md`; modify `.github/workflows/ci.yml`, `README.md` (one line).

- [ ] **Step 1: Write the submission.** `submission.toml`: `schema_version = 1`, `kind = "thrust_stand"`, `title = "Illustrative 2207 1950 KV motor, 5-inch propeller, 6S (synthetic)"`, `quad = "../../../quads/opendrone-5f-freestyle.toml"`, `motor = 0`, `data = "thrust.csv"`, `[origin]` `source = "estimated"`, `note = "Hand-written, datasheet-style figures that show the format and the report. NOT a measurement of any motor."`, `contributor = "Open FPV Sim maintainers"`, `license = "CC-BY-SA-4.0"`. `thrust.csv`: the six rows of design call 13 (a blank current cell on the `0.30` row).
- [ ] **Step 2: Generate the report.** `cargo run -p ofs-validate --locked -- validation/thrust-stand/example-2207-5in --out validation/thrust-stand/example-2207-5in/REPORT.md`; read it; check that it states the data are estimated and that the disagreement it shows is plausible (the model spins faster and draws less than the synthetic rows: if the opposite, stop and tell the maintainer, because that is a model finding). `cargo test -p ofs-validate --test example --locked` now passes.
- [ ] **Step 3: Write the docs.** `docs/validation.md`: why (parent spec §8.5), the folder layout, `submission.toml` keys with units, the CSV columns with an example, the Blackbox kind and what `blackbox_decode` command produces it, how to run `ofs-validate`, how to read a report, what "estimated / measured / datasheet" mean (same vocabulary as library provenance), the licence (CC-BY-SA-4.0, as the library), and how a contributor opens a PR (data + report; the maintainers then update the part's `[provenance]` from `estimated`). `validation/README.md`: two paragraphs pointing at it.
- [ ] **Step 4: CI.** In the `core` job after `cargo test`: `- run: cargo run -p ofs-validate --locked -- validation/thrust-stand/example-2207-5in --summary` with a comment: tracked, not gating (parent spec §8.5): it fails only when a shipped submission is invalid or the report cannot be produced.
- [ ] **Step 5: Verify and commit.** `cargo test --workspace --locked`; run the command from Step 4 locally with `GITHUB_STEP_SUMMARY` pointing at a scratch file and read it. Commit `feat(validate): a worked thrust-stand report, the format doc and a CI summary (#49)`.

---

### Task 7: Phase close

**Files:** modify `docs/roadmap.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `docs/architecture.md`, `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md` (§8.5 and §9 tables), `docs/superpowers/specs/2026-10-13-phase2-real-gear-trusted-numbers-design.md` (status line).

- [ ] **Step 1: Roadmap.** Replace the "M6 is in progress" paragraph with "Phase 2 (M5, M6) is done" and a sentence on what it delivered; remove "The repro bundle" from "Sandbox and component library" and the thrust-stand line from "Validation" (leave "simulated against real Blackbox step responses" there: the format exists, the comparator waits for data); add under Validation: "Real measurements: the replacement for every `estimated` constant. The format is `docs/validation.md`." Add the Rulings: **the repro bundle replays only what steers a run** (not Betaflight's nondeterminism in real time) and **the thermal and cell constants stay estimates until a validation submission replaces them**.
- [ ] **Step 2: Parent spec.** §8.5: mark "built (thrust-stand comparator, Blackbox format): `docs/validation.md`"; §9: add the M5 and M6 rows with their merge commits (M5 `b734a7f`, M6a `8377a86`, M6b `2ea9966`, M6c: fill in at merge).
- [ ] **Step 3: Changelog, contributing, architecture.** `CHANGELOG.md` `## Unreleased — M6c` (Added: repro bundle, `--replay`, `ofs.repro`, the rolling tail, `ofs-validate`, the submission format and example; Changed: `SimError` events now end with the bundle path). `CONTRIBUTING.md`: the validation-data route and the repro-bundle route in two short paragraphs. `docs/architecture.md`: the `ofs-validate` crate row and the bundle/journal in the data-flow section.
- [ ] **Step 4: Verify.** `cargo test --workspace --locked`, clippy, the Python suite, `cargo +1.85 check --workspace --all-targets --locked --exclude ofs-godot`; every link in the new docs resolves (`grep` the relative paths); run the one-line replay example from `docs/repro-bundle.md` against a fresh bundle exactly as printed. WSL Linux full `cargo test --workspace` and the live `sitl_live` tests (10) once at the end.
- [ ] **Step 5: Commit** `docs: M6c done, phase 2 closed`.

---

## Out of scope

The Blackbox step-response comparator and the RPM-filter spectral proof (they need real logs and SITL; the format is ready for them); an archive form of the bundle; replaying a *real-time* session exactly; tail windows other than 10 s; a bundle for config errors; ticking the epic #73 checklist.
