# M6d: Phase-2 Deferred Minors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the minors the whole-branch reviews of M6a, M6b and M6c deferred: no new features, no model changes, every radio golden digest unchanged.

**Architecture:** Eight small, independent tasks. Tasks 1-5 are the M6c list (the repro bundle and `ofs-validate`); tasks 6-8 are the cheap M6a/M6b items (config range checks, stale comments, weak tests). Each task is one commit and touches its own files.

**Tech Stack:** Rust (workspace, MSRV 1.85), Python `ofs`, Betaflight SITL (WSL) for task 5.

**Spec:** none. The scope is the "Deferred minors" lists in the bodies of PR #78, #79 and #80 (`gh pr view <n>`); the formats they touch are in `docs/repro-bundle.md` and `docs/validation.md`.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows.
- The radio goldens (`crates/ofs-radio/tests/golden/elrs.{windows,linux}.digests`, `crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`) must not change. A task that moves one has gone beyond a minor: stop and rule.
- `cargo fmt` is not a gate; do not reformat existing files.
- `cargo +1.85 check` passes on every changed crate.
- A worktree-isolated session refuses compound shell commands, heredocs and WSL through Bash: use PowerShell for WSL, Edit/Write for files. After a dependency change run `cargo ... --offline` once so `Cargo.lock` updates.
- Every new test is run red first. Where a task changes behaviour a test already pins, say so in the commit message.

## Review Focus

- A journal written on Windows by hand or by a text editor starts with a byte-order mark or has CRLF endings: the header check must accept both (task 1).
- A truncated journal (`# truncated` line after the header) must still read (task 1).
- A bundle folder that is still being written has no `bundle.toml` yet: `find_bundles` must not list it (task 4).
- A replay error whose text holds non-ASCII (a path with an accent) must come back readable on Windows (task 4).
- A `--max-error-pct` gate must fail when a row never settled, but a run without the flag must still exit 0 (task 3).
- A TOML error with no span must still be reported, just without a line (task 2).

---

### Task 1: `Journal::read` checks its header

**Files:**
- Modify: `crates/ofs-sim/src/journal.rs` (`Journal::read`, near line 105; `HEADER` is line 13)
- Test: `crates/ofs-sim/tests/journal.rs`

**Interfaces:**
- Consumes: `HEADER: &str = "# ofs-journal 1"`, `Journal::read(dir: &Path) -> Result<Journal, String>`.
- Produces: the same signature. `read` now returns `Err` when the first non-blank line is not `HEADER`.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-sim/tests/journal.rs` (use the file's existing temp-dir helper):
  - `a_journal_without_the_header_is_refused`: a `journal.txt` holding only `1 tx\n`; `read` is `Err` and the message contains `journal.txt` and `ofs-journal`.
  - `a_journal_of_another_version_is_refused`: header `# ofs-journal 2`; `Err` whose message names the version found and the version this build reads (`1`).
  - `a_header_with_a_byte_order_mark_and_crlf_reads`: the text `"\u{feff}# ofs-journal 1\r\n1 tx\r\n"` reads and has one entry.
  - `a_truncated_journal_still_reads`: header, one entry, then `# truncated`; `read` is `Ok` and `truncated()` is true.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test journal` — Expected: the first two FAIL (they read as `Ok`), the other two pass.
- [ ] **Step 3: Implement** the check in `Journal::read`: strip a leading `\u{feff}`, find the first non-blank line, and require it to equal `HEADER` after `trim()`. A different `# ofs-journal N` reports `journal.txt: written by format {N}, this build reads 1`; anything else reports `journal.txt: line {n}: not a journal (expected "{HEADER}")`. Keep the per-line loop as it is.
- [ ] **Step 4: Run** `cargo test -p ofs-sim` — Expected: all pass, including `tests/repro.rs` (its `change_the_journal` helper edits a real journal, so it proves the header survives).
- [ ] **Step 5: Commit** `fix(sim): a journal must open with its header`.

---

### Task 2: A `submission.toml` syntax error names its line

**Files:**
- Modify: `crates/ofs-validate/src/submission.rs` (`load`, line 128)
- Test: `crates/ofs-validate/tests/submission.rs`

**Interfaces:**
- Produces: `load` keeps its signature. A TOML parse or schema error is reported as `submission.toml line {n}: {message}` when the error has a span, and `submission.toml: {message}` when it has none.

- [ ] **Step 1: Write the failing tests** (`tests/submission.rs`, using its existing helper that writes a folder):
  - `a_toml_type_error_names_its_line`: line 1 `schema_version = 1`, line 2 `kind = "thrust_stand"`, line 3 `motor = "zero"`; the single problem starts with `submission.toml line 3:`.
  - `an_unknown_key_names_its_line`: an unknown key `colour = 1` on line 4 gives a problem starting `submission.toml line 4:`.
- [ ] **Step 2: Run** `cargo test -p ofs-validate --test submission` — Expected: FAIL, the message has no line.
- [ ] **Step 3: Implement** a private `fn toml_error(text: &str, e: &toml::de::Error) -> String`: `e.span()` gives a byte range; the line is the count of `\n` in `text[..span.start]` plus one (a BOM-stripped text is fine because the BOM is not a newline). Use it in `load`.
- [ ] **Step 4: Run** `cargo test -p ofs-validate` — Expected: pass.
- [ ] **Step 5: Commit** `fix(validate): a submission.toml error names its line`.

---

### Task 3: The error gate counts rows that never settled; the settle threshold is per second

**Files:**
- Modify: `crates/ofs-validate/src/report.rs` (add `unsettled`), `crates/ofs-validate/src/main.rs` (the gate, lines 67-70), `crates/ofs-validate/src/thrust.rs` (`SETTLED`, line 17, and its use, line 57)
- Test: `crates/ofs-validate/tests/report.rs`, `crates/ofs-validate/tests/cli.rs`, `crates/ofs-validate/tests/thrust.rs`
- Regenerate: `validation/thrust-stand/example-2207-5in/REPORT.md` if the figures move

**Interfaces:**
- Produces: `pub fn unsettled(rows: &[Compared]) -> usize` in `report.rs` (rows whose prediction is `Err`).
- `SETTLED` becomes a per-second fraction: the test is `|w - previous| <= SETTLED * BLOCK_S * w.max(1.0)`, the plan-of-record meaning (`1e-6` per second).

- [ ] **Step 1: Write the failing tests**
  - `report.rs`: `unsettled_counts_the_rows_with_an_error` (two rows, one `Err`, expect 1).
  - `cli.rs`: `a_gate_fails_when_a_row_never_settles` — a submission whose row asks for an impossible state (the existing `thrust.rs` tests show how a row fails to settle; reuse that command/voltage); run with `--max-error-pct 1000` and expect exit 1 and a stderr line naming the unsettled count. `no_gate_means_exit_0_even_with_an_unsettled_row`: the same folder without the flag exits 0.
  - `thrust.rs`: `a_run_that_still_moves_is_not_settled` — after the change the existing steady-state tests must still pass, and `settle_s` for the example's 0.20 row is a multiple of `0.25`.
- [ ] **Step 2: Run** `cargo test -p ofs-validate` — Expected: the two `cli.rs` tests and `unsettled_counts...` FAIL.
- [ ] **Step 3: Implement** `unsettled`; in `main.rs` fail with exit 1 when `args.max_error_pct.is_some()` and `unsettled(rows) > 0`, printing `ofs-validate: {n} row(s) did not settle, so the gate cannot pass`; apply the per-second threshold. Keep exit code 2 for invalid submissions.
- [ ] **Step 4: Regenerate and diff the worked report**: run `cargo run -p ofs-validate --locked -- validation/thrust-stand/example-2207-5in --out validation/thrust-stand/example-2207-5in/REPORT.md`. If `tests/example.rs` compares the file with the generated text, the regenerated file is part of this commit. Record in the commit message which cells moved, if any.
- [ ] **Step 5: Run** `cargo test --workspace --locked` — Expected: pass.
- [ ] **Step 6: Commit** `fix(validate): the error gate counts rows that never settled; settle threshold per second`.

---

### Task 4: `ofs.repro` — whole bundles only, UTF-8 output

**Files:**
- Modify: `python/ofs/repro.py` (`find_bundles` line 30, `replay_bundle` line 47)
- Test: `python/tests/test_repro.py`

**Interfaces:**
- Produces: the same signatures. `find_bundles` lists only folders that end in `.repro` and contain a `bundle.toml`. `replay_bundle` runs `subprocess.run(..., capture_output=True, text=True, encoding="utf-8", errors="replace")`.

- [ ] **Step 1: Write the failing tests**
  - `test_find_bundles_skips_a_folder_without_bundle_toml`: in `tmp_path` make `a-1.repro/` with a `bundle.toml` and `a-2.repro/` without; only `a-1.repro` is listed. (The existing `test_find_bundles_lists_only_bundle_folders_in_order` creates bare folders: update it to add `bundle.toml` to each.)
  - `test_a_replay_error_with_non_ascii_text_reads_back`: replay of a folder named `défaut-1.repro` that does not exist; the raised `ConfigError` message contains `défaut`. Use `sim_bin` like the neighbouring test.
- [ ] **Step 2: Run** `python -m pytest python/tests/test_repro.py -q` — Expected: the first FAILs; the second FAILs on Windows (garbled text) and may pass on Linux, which is fine.
- [ ] **Step 3: Implement** both changes.
- [ ] **Step 4: Run** `python -m pytest python/tests -q` — Expected: pass (65 passed, 23 SITL-skipped before this task).
- [ ] **Step 5: Commit** `fix(python): find_bundles skips partial folders; replay output is UTF-8`.

---

### Task 5: A live test for a real SITL crash

**Files:**
- Modify: `crates/ofs-sim/tests/sitl_live.rs` (add one `#[ignore]` test; CI's `sitl` job already runs `--test sitl_live -- --ignored`)

**Interfaces:**
- Consumes: the gRPC helpers `start()` and the load/run pattern of `crates/ofs-sim/tests/repro.rs` (`fail_a_session`), `ofs_sim::repro::replay`, the SITL quad `quads/opendrone-5f-freestyle.toml` with `open_loop_fc: false`, `pkill -9 -x betaflight_SITL` (Linux only).

- [ ] **Step 1: Write the test** `a_sitl_crash_through_the_session_leaves_a_replayable_bundle`. Copy the small `start()` helper (do not make `repro.rs` public). Load the SITL quad in lockstep, run 2 s of armed flight, kill SITL from outside (`std::process::Command::new("pkill").args(["-9", "-x", "betaflight_SITL"])`), then `Run` again and expect the error. Assert: the `sim_error` event text contains `(repro bundle: `; the folder has `firmware/eeprom.bin`, `firmware/sitl.log`, `tail.mcap`, `journal.txt`; `replay(&folder, scratch)` returns `Ok` with `outcome.failure.is_none()` (the external kill is not in the journal, so it cannot recur) and `!outcome.reproduced`.
- [ ] **Step 2: Run in WSL** (PowerShell): `wsl -d Ubuntu -- bash -lc "cd /mnt/c/dev/open-fpv-sim && cargo test -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1 a_sitl_crash"` using the repo's WSL command from `docs/dev-setup.md` and the memory note on SITL paths. Expected: PASS. The first run proves the path exists, so also prove the assertion can fail: temporarily drop the `Run` after the kill and see the test fail on the missing `sim_error` event; restore.
- [ ] **Step 3: Run the whole file** with `--test-threads=1` — Expected: all live tests pass (no port-bind flake; if it happens, re-run once and say so).
- [ ] **Step 4: Commit** `test(sim): a SITL crash through the session leaves a replayable bundle`.

---

### Task 6: Config guards

**Files:**
- Modify: `crates/ofs-config/src/lib.rs` (the `props` check at line 1007; the battery checks at lines 1037-1045), `crates/ofs-config/src/world.rs` (line 341)
- Test: `crates/ofs-config/tests/config.rs`

**Interfaces:**
- Produces: three new validation rules, all reported as `Problem { field, message }` like the existing ones.

- [ ] **Step 1: Write the failing tests** (each follows `windmill_j0_must_not_be_negative`, line 134):
  - `windmill_j0_must_not_be_tiny`: `windmill_j0 = 0.01` is rejected on field `props[0].windmill_j0`; `0.0` (off) and `1.0` (shipped) load.
  - `an_aged_pack_may_not_exceed_ten_times_the_resistance`: `resistance_growth_per_cycle = 0.1` with `cycles = 100` (factor 11) is rejected on `battery.resistance_growth_per_cycle`; the shipped aged pack still loads.
  - In `crates/ofs-config/tests/world.rs` (or where the `[video]` tests live): `a_non_finite_video_threshold_names_its_field` — `sparkle_snr_db = nan` is reported on `video.sparkle_snr_db`, not on `video`.
- [ ] **Step 2: Run** `cargo test -p ofs-config` — Expected: the three FAIL.
- [ ] **Step 3: Implement**
  - `windmill_j0`: `0.0` or `>= 0.1`. Ruling for the floor: below it the windmill torque `Cp·(1 − J/j0)` swings negative over the first fraction of a unit of advance ratio and drives the rotor (PR #78 saw a runaway at 0.01); confirm 0.1 is safe with the existing `crates/ofs-sim/tests/windmill.rs` harness at `j0 = 0.1`, and ledger the figure.
  - Resistance: `1.0 + resistance_growth_per_cycle * cycles <= 10.0`, reported on the growth field with the cycle count in the message.
  - Video: check each threshold's finiteness in its own `c.check(v.is_finite(), "video.<name>", "must be finite")` instead of the single `"video"` check; keep the ordering checks inside the `else` as they are.
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass (shipped quads, library and worlds all still load).
- [ ] **Step 5: Commit** `fix(config): reject a runaway windmill_j0, an over-aged pack and name non-finite video fields`.

---

### Task 7: Stale comments and one roadmap sentence

**Files:**
- Modify: `proto/ofs/v1/sim.proto:216`, `python/ofs/client.py:221`, `crates/ofs-sim/src/vehicle.rs:257` (and the misplaced doc comment on `Vehicle::signal`), `docs/roadmap.md` (the physics-limits row near line 96)

- [ ] **Step 1: Edit the proto and client comments** to `per-cell voltage (each cell's own state; the weak cell droops)`. The proto change is a comment only: regenerate nothing, and confirm `git diff --stat` shows no generated file.
- [ ] **Step 2: Fix `vehicle.rs`**: the line-257 doc says "equal shares"; replace it with the truth (per-cell voltages come from the per-cell battery model, the weak cell carrying its droop). Move the doc comment that `Vehicle::signal` took over from `world()` back onto `world()` (read both items and decide which sentence belongs to which).
- [ ] **Step 3: Reword the roadmap row**: add back, in the physics limitations, that the IMU's analytic vibration-rate term still ignores gyroscopic coupling (true as of M6a; read `docs/research/rotor-dynamics.md` first so the sentence agrees with it).
- [ ] **Step 4: Run** `cargo test --workspace --locked` and `python -m pytest python/tests -q` — Expected: pass (comments only).
- [ ] **Step 5: Commit** `docs: stale cell-voltage comments, vehicle doc comments, roadmap vibration-rate limit`.

---

### Task 8: Make three weak tests real guards

**Files:**
- Modify: `crates/ofs-sim/tests/open_loop_vehicle.rs` (`mixed_pole_pairs_fly_finite_in_open_loop`, line 168), `crates/ofs-sim/tests/windmill.rs` (`a_dying_motor_in_a_descent_changes_speed_without_a_step`, line 73), and the weak-cell range-check test in `crates/ofs-config/tests/config.rs` (find it with `grep -n "cell" crates/ofs-config/tests/config.rs`)

- [ ] **Step 1: For each test, name the production change that must make it fail** (the first: a pole-pair count ignored in the electrical-to-mechanical speed; the second: windmilling disabled for a dead motor, and a one-tick jump in `motor.omega`; the third: the weak-cell range check removed). Write the mutation, run the test, and record whether it fails.
- [ ] **Step 2: A test that stays green under its mutation is rewritten** until it fails: assert a number (an rpm ratio between the two pole-pair counts from first principles; a bound on the largest single-tick change of `motor.omega` against the steady descent decay; the exact rejected field name). Revert each mutation after the check.
- [ ] **Step 3: Run** `cargo test --workspace --locked` — Expected: pass with the mutations reverted (`git diff` shows only test files).
- [ ] **Step 4: Commit** `test: the three weak M6 tests now fail when the behaviour they guard is removed`.

---

## Not in this plan

- Default recordings carry the 100 Hz thermal signals at the 240 Hz `esc.*`/`motor.*` rate: a rate-table change alters recorded bytes and, through the whole-vehicle MCAP digest, the radio goldens. It needs a deliberate regeneration on both OSes; ask first.
- Standalone rigs without a thermal model (`ofs-fc` live ESC rig, a Tramp-only rig) send 0 °C: only test rigs, `build()`-ed vehicles are fine.
- The capacity-fade boundary is float-fuzzy and `Battery::new` does not assert positive factors: the config check is the gate; revisit only with a real report.

## Self-Review

- **Coverage:** M6c list (header, TOML line, unsettled gate, settle threshold, UTF-8, partial folders, live SITL crash) is tasks 1-5; M6b list tasks 6-7 (video field name, resistance bound, stale comments, doc comment); M6a list tasks 6-8 (`windmill_j0`, weak tests, roadmap sentence). The three items under "Not in this plan" are decisions, not omissions.
- **Placeholders:** none; tasks 3, 5, 6 and 8 leave one number or one file lookup to the implementer, each with the rule that fixes it.
- **Interfaces:** `unsettled` is produced in task 3 and used only there; `Journal::read` keeps its signature.
