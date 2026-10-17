# M7a: Hardening Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the eight open robustness issues #54, #55, #58, #57, #14, #59, #13 and #12 (no new features) so sessions end cleanly, bad input is refused instead of crashing a run, and the radio model matches ExpressLRS's published table.

**Architecture:** Nine small tasks, mostly independent, one commit each (the two golden regenerations get their own commits). Rust fixes are test-first; the SITL patch change (#57) is test-first against real Betaflight in WSL; #14 turns out to be already fixed in the patch and is closed with evidence.

**Tech Stack:** Rust workspace (MSRV 1.85), Python `ofs`, Betaflight SITL 2026.6.2 + `third_party/betaflight/ofs-sitl.patch` (C), WSL Ubuntu for live tests.

**Spec:** none. Scope is the bodies of GitHub issues #54, #55, #58, #57, #14, #59, #13, #12 (`gh issue view <n> --repo hugowis/open-fpv-sim`), `docs/roadmap.md` and `docs/superpowers/history/closeout-evidence.md`.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `cargo +1.85 check` passes on every changed crate.
- Radio goldens (`crates/ofs-radio/tests/golden/elrs.{windows,linux}.digests`, `crates/ofs-sim/tests/golden/radio-flight.{windows,linux}.digests`) change **only** in tasks 6 and 7, each in its own commit, regenerated on both OSes with `OFS_GOLDEN_WRITE=1` (Linux in WSL Ubuntu 24.04, the CI runner's glibc), and the commit message says why. Any other task that moves a golden has a bug.
- `cargo fmt` is not a gate; do not reformat existing files.
- Worktree-isolated sessions refuse heredocs and WSL through Bash: use Write/Edit for files and PowerShell for WSL. For multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>` (quoting through PowerShell mangles pipes and `$`). After a dependency change run one `--offline` build.
- Commit trailers: a blank line, then `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` on its own line.
- Live SITL commands (WSL): `CARGO_TARGET_DIR=$HOME/ofs/target OFS_SITL_LAUNCH=$HOME/ofs/betaflight/obj/main/betaflight_SITL.elf cargo test -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1`; run `pkill -9 -x betaflight_SITL` first if a port is busy.

## Design calls (decided here; the executor ledgers any change)

1. `Vehicle::set_sticks` becomes `-> Result<(), SimError>`: a non-finite value is refused with `SimError::InvalidArgument` and nothing is applied, journaled or recorded. Accepted values are clamped (every stick to [-1, 1], throttle to [0, 1]) **before** they reach the bus, the recorder and the journal, so a replay sees what the bus saw.
2. A session that ends armed spends up to 0.5 simulated seconds disarming Betaflight (all aux low, throttle 0, written straight to the bus, not through `set_sticks`) so its Blackbox log is finished. Those steps are not journaled and not written to an inputs recording; a running MCAP recording does see them. A dead or failed SITL is skipped without waiting.
3. #57 is fixed at its root: Betaflight waits for the MSP reply to leave its TX buffer before rebooting (`waitForSerialPortToFinishTransmitting`), and the patched `tcpDataOut` returns early when no client is connected without draining that buffer, so a client that hung up leaves it full forever. The patch will drop the bytes when there is no connection (a real UART with nobody listening does the same).
4. #14 needs no code: the patch already has the bounded retry (`ac97cad`) and the root-cause fix (`6ccbaef`). The task verifies that and closes the issue.
5. #12: the figure is -115 dBm, from ExpressLRS's signal-health table (https://expresslrs.org/info/signal-health/, "ExpressLRS 4.x / 3.x" table, fetched 2026-10-17; the same table matches the model's other 2.4 GHz and 900 MHz rows).
6. #13: the burst test snaps the phase by 1 ns at both edges (`on` and the period).

## Review Focus

- A refused `set_sticks` leaves the previous sticks on the bus, in the journal and in the recording untouched (task 1).
- An out-of-range stick clamps identically on the bus, in `play_inputs`/recorded CSV and in a replayed journal (task 1).
- A session whose SITL already died, or whose scheduler failed, ends without hanging or panicking in the disarm (task 4).
- The disarm at session end adds no row to an inputs recording and no entry to the journal (task 4).
- `Unload`/`Load` with no inputs recording running publishes no `inputs_recording_stopped` event (task 3).
- Reading a recording that was never finished gives an error that says so and says what to do (task 5).

---

### Task 1: `set_sticks` refuses non-finite input and clamps what it accepts (#54, bullets 1-2)

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (`Sticks`, `set_sticks` at line 1029, `Action::Sticks` replay at line 1351, its other callers), `crates/ofs-sim/src/server.rs:299`, `crates/ofs-sim/src/streams.rs:199,226`, `crates/ofs-sim/src/repro.rs:366,522`, `crates/ofs-godot/src/lib.rs:120` (check it is a client call, not `Vehicle`), and every test calling `.set_sticks(` (73 sites: `grep -rn "\.set_sticks(" crates --include=*.rs`)
- Test: `crates/ofs-sim/tests/open_loop_vehicle.rs` (or a new `tests/sticks.rs`)

**Interfaces:**
- Produces: `impl Sticks { pub fn clamped(&self) -> Sticks }` (roll/pitch/yaw/aux to [-1, 1], throttle to [0, 1]; only for finite input) and `Vehicle::set_sticks(&mut self, s: &Sticks) -> Result<(), SimError>`.
- Consumes: `SimError::InvalidArgument(String)`.

- [ ] **Step 1: Write the failing tests** in `crates/ofs-sim/tests/open_loop_vehicle.rs` (use its `vehicle_in`/`throttle` helpers):
  - `set_sticks_refuses_a_non_finite_value_and_changes_nothing`: for each of NaN and ±inf in roll, throttle and an aux channel, `set_sticks` returns `Err(SimError::InvalidArgument(_))` whose text names the field; afterwards `v.signal("rc.throttle")` (use the names in `ofs_core::names`) still holds the previous value and `v.journal().entries()` has not grown.
  - `set_sticks_clamps_what_it_accepts`: `Sticks { roll: 3.0, pitch: -3.0, throttle: 2.0, aux: [5.0, -5.0, 0.5, 0.0], .. }` puts exactly `1.0, -1.0, 1.0` and `1.0, -1.0, 0.5, 0.0` on the bus, and the journal's `Action::Sticks` carries the clamped values; a throttle of `-1.0` becomes `0.0`.
  - `a_replayed_journal_sees_the_clamped_sticks`: fly with an out-of-range stick, build a second vehicle, `replay_action` every journal entry, and compare `state().pos_ned_m` bit for bit.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test open_loop_vehicle sticks` — Expected: FAIL (compile error: `set_sticks` returns `()`; then the assertions).
- [ ] **Step 3: Implement** `Sticks::clamped` and the new `set_sticks` (validate all eight values first, apply `clamped()` to the bus, the recorder and the journal; return `Err` before any side effect). Make every production caller handle the `Result`: `server.rs` maps it to the gRPC `invalid_argument` status it already uses for non-finite sticks (keep its own earlier check or delete it as redundant, your call); `streams.rs` and the replay arm propagate with `?` or log-and-skip as their context needs; tests get `.unwrap()` (a sed over single-line calls `s/(\.set_sticks\(.*\));$/\1.unwrap();/`, then fix the multi-line ones the compiler lists). The two tests that used `set_sticks(NaN)` to poison a session (`server.rs` near line 795, `session.rs` near line 705) switch to a scheduled/applied `Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 1e305 }` with `v.run_for(...)` throwing `SimError::NonFinite` (the pattern `crates/ofs-sim/tests/repro.rs::fail_a_session` uses).
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: PASS.
- [ ] **Step 5: Commit** `fix(sim): set_sticks refuses non-finite sticks and clamps the rest (#54)`.

---

### Task 2: The inputs recorder writes no redundant row and always writes a first row (#54, bullets 3-4)

**Files:**
- Modify: `crates/ofs-sim/src/inputs_recorder.rs` (`values_of` line 41, `record` line 76, `write_pending` line 92)
- Test: the same file's `mod tests`

**Interfaces:**
- Produces: `values_of(&Sticks) -> [f64; 8]` (total: a non-finite field maps to that field's `Sticks::default()` value), `InputsRecorder` keeps `committed: Option<[f64; 8]>` (the newest row actually written).

- [ ] **Step 1: Write the failing tests** in `inputs_recorder.rs`'s `mod tests` (the file has `sticks(throttle)` and `over(...)` helpers): `a_change_reverted_in_the_same_tick_writes_no_row` (record throttle 0.1 at tick 0, 0.5 at tick 10, 0.1 again at tick 10, then 0.2 at tick 20: the file's rows are exactly the tick-0 row and the tick-20 row, `summary.rows == 2`); `a_recording_started_on_a_non_finite_stick_still_has_its_first_row` (start with `throttle: f64::NAN`: row 1 exists, with throttle 0 and aux -1).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib inputs_recorder` — Expected: FAIL (3 rows written; no first row).
- [ ] **Step 3: Implement**: in `record`, when the same-tick branch replaces `pending`, drop `pending` altogether if the new values equal `committed`; `write_pending` sets `committed`. Make `values_of` total and delete the `Some/None` plumbing.
- [ ] **Step 4: Run** `cargo test -p ofs-sim` — Expected: PASS (the M5 `tests/inputs_recording.rs` stays green).
- [ ] **Step 5: Commit** `fix(sim): the inputs recorder drops a reverted change and always writes a first row (#54)`.

---

### Task 3: Ending a session mid-recording publishes `inputs_recording_stopped` (#55)

**Files:**
- Modify: `crates/ofs-sim/src/server.rs` (`load` at line 273, `unload` at line 405; `stop_inputs_recording` at line 571 for the message format)
- Test: the `mod tests` of `server.rs` (it already has `a_failure_at_the_end_of_an_inputs_recording_still_publishes_its_stop_event`, line 623) or `crates/ofs-sim/tests/inputs_recording.rs`

**Interfaces:**
- Produces: a private `fn end_session(shared: &Shared, slot: &mut Slot)` that, if the loaded session's vehicle `is_recording_inputs()`, stops the recording (so the file is complete) and publishes `InputsRecordingStopped` with the same message `stop_inputs_recording` uses (`"<path>: <rows> rows over <s> s"`, or `"inputs recording failed: <why>"`), then sets the slot to `None`. `load` and `unload` call it instead of `*slot = None`.

- [ ] **Step 1: Write the failing tests**: `unload_mid_recording_publishes_the_stop_event` and `load_mid_recording_publishes_the_stop_event` (start a recording on a loaded open-loop session, subscribe with `svc.subscribe()`, call `unload`/`load`, expect exactly one `InputsRecordingStopped` whose message names the CSV path and the file exists and ends with its last row); `unload_without_a_recording_publishes_no_stop_event`.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib server` — Expected: FAIL (no event).
- [ ] **Step 3: Implement** `end_session` and use it in both handlers.
- [ ] **Step 4: Run** `cargo test -p ofs-sim` — Expected: PASS. Ruling to ledger: the Godot workaround (`godot/scripts/app.gd:257`, `inputs_recording = false` on `loading/failed/stopped`) stays: it also covers a lost session, for which no event exists.
- [ ] **Step 5: Commit** `fix(sim): Load and Unload publish inputs_recording_stopped (#55)`.

---

### Task 4: A session that ends armed leaves a complete Blackbox log (#58)

**Files:**
- Modify: `crates/ofs-sim/src/vehicle.rs` (new `Vehicle::disarm_for_shutdown`), `crates/ofs-sim/src/session.rs` (new `impl Drop for Session`; the `blackbox` field stays declared last)
- Test: `crates/ofs-sim/tests/sitl_live.rs` (live, `#[ignore]`, Linux), plus a unit test for the no-SITL no-op

**Interfaces:**
- Produces: `Vehicle::disarm_for_shutdown(&mut self)`: a no-op unless a SITL bridge is running; otherwise writes aux channels -1 and throttle 0 straight to the bus (no journal entry, no inputs-recorder row), then `run_for(0.5)` ignoring any error (a dead SITL returns at once). `Session`'s `Drop` calls it when the session is not poisoned.

- [ ] **Step 1: Write the failing tests.** Unit (`vehicle.rs` tests): `disarm_for_shutdown_is_a_no_op_without_sitl` (open-loop: time and journal unchanged) and `disarm_for_shutdown_leaves_the_journal_and_the_inputs_recording_alone`. Live (`sitl_live.rs`, patterned on `a_sitl_crash_through_the_session_leaves_a_replayable_bundle`, `#[cfg(target_os = "linux")]`): `a_session_that_ends_armed_leaves_a_finished_blackbox_log`: load the SITL quad over gRPC, run 4 s with the arm switch low, raise it, run 2 s and assert armed (`motor_cmd` all > 0), `Unload`, then find `<logs>/*-LOG*.BFL` and assert its bytes contain `End of log` (the Blackbox end-of-log event, written when logging stops on disarm) and that the file is larger than one run's worth of a disarmed session (> 1 KiB).
- [ ] **Step 2: Run** the live test in WSL — Expected: FAIL (`End of log` absent); the unit tests FAIL (no method).
- [ ] **Step 3: Implement** the method and the `Drop`. If `impl Drop for Session` breaks code that moves fields out of a `Session`, restructure that code rather than adding `Option` wrappers. Document the 0.5 s in `docs/dev-setup.md` where it says "disarm first" and in `docs/roadmap.md` (remove the known-limitation bullet).
- [ ] **Step 4: Run** the live file and `cargo test -p ofs-sim` — Expected: PASS. Also run once with the test's final step skipped (SITL killed with `pkill -9` before `Unload`): the session still ends without a hang (this is the "dead SITL" Review Focus line; add it as `a_session_whose_sitl_died_still_ends_promptly` if it is cheap, asserting `Unload` returns within 5 s).
- [ ] **Step 5: Commit** `fix(sim): a session that ends armed disarms Betaflight first so its Blackbox log is finished (#58)`.

---

### Task 5: A recording that stops on a write error is finished if the sink allows, and says so if not (#59)

**Files:**
- Modify: `crates/ofs-log/src/recorder.rs` (`record`, line 234: the failure branch), `python/ofs/reader.py:71`
- Test: `crates/ofs-log/tests/recorder.rs` (its `Failing` writer is at line 93), `python/tests/test_recording.py`

- [ ] **Step 1: Spike (10 minutes, ledger the result).** In a scratch test, write a `Flaky` writer (`Write + Seek` over a `Cursor`) that fails exactly one write once the buffer passes a threshold and succeeds afterwards, drive the recorder into the failure, then call `sink.finish()` on the sink. Does the pinned mcap writer produce a file whose summary `mcap::Summary::read` finds? If **yes** go on with the best-effort finish (steps 2-4); if **no** (the writer is left inconsistent), ledger `Ruling: no best-effort finish: the pinned mcap writer cannot finish after a failed chunk write` and do only the reader message (step 3b).
- [ ] **Step 2: Write the failing tests.** Rust (`tests/recorder.rs`): `a_recording_that_stops_on_a_transient_write_error_is_finished` using `Flaky`: after the failure the file has a summary and the messages written before the failure; `a_recording_that_stops_on_a_permanent_error_does_not_panic` (the existing `Failing` case; the failure text is still the original write error even if finishing also fails). Python (`test_recording.py`): `test_reading_an_unfinished_file_says_what_happened`: write a truncated MCAP (take a finished recording's bytes without its last 100 bytes) and expect `ValueError` whose message contains `unfinished` and `recording_stopped`.
- [ ] **Step 3: Implement** (a) in `record`'s failure branch, `let _ = active.sink.finish()` best-effort **before** dropping it, never replacing the original error; (b) `reader.py`: the "not a finished recording" error becomes `"{path}: unfinished recording (it was stopped by a write error such as a full disk, or the session ended without stopping it); the recording_stopped event names the cause"`.
- [ ] **Step 4: Run** `cargo test -p ofs-log` (the determinism test `tests/determinism.rs` must stay green) and `python -m pytest python/tests/test_recording.py -q` (with `PYTHONPATH=<worktree>/python`) — Expected: PASS.
- [ ] **Step 5: Commit** `fix(log): finish a recording that stopped on a write error if possible; name an unfinished file (#59)`.

---

### Task 6: The burst-loss window no longer loses or keeps a packet at an edge (#13)

**Files:**
- Modify: `crates/ofs-radio/src/parts.rs:160` (`FaultSignals::survives`)
- Test: `crates/ofs-radio/tests/` (a new `burst_edges.rs`); goldens if they move

**Interfaces:**
- Produces: the same `survives(&self, bus, time_s, fault_draw) -> bool`; the burst test becomes `phase < on - EDGE_S` with `phase` snapped to 0 when within `EDGE_S` (1e-9) below `on + off`, i.e. a packet within a nanosecond of an edge belongs to the window that starts there.

- [ ] **Step 1: Write the failing test** `burst_windows_follow_integer_packet_arithmetic` in `crates/ofs-radio/tests/burst_edges.rs`: build a `Bus`, `FaultSignals::register`, set `fault.radio.burst_on_s = 0.05`, `burst_off_s = 0.05` (use the `names::FAULT_RADIO_BURST_*` constants); for packet `k` in `0..5000` at `time_s = k as f64 * 0.004` (250 Hz) the packet is lost iff `k % 25 < 13` (12.5 packets on, 12.5 off: the edge at `on` falls between packets, the period edge on a packet); assert `survives(.., 1.0 /* draw above any fraction */)` equals `k % 25 >= 13` for all k. A second case `on = off = 0.04` at 250 Hz: lost iff `k % 20 < 10`. Run it **first against the unchanged code** and record which k fail (the issue saw packet 75).
- [ ] **Step 2: Run** `cargo test -p ofs-radio --test burst_edges` — Expected: FAIL at the packets that sit on a period edge (e.g. k = 75, 100, ...).
- [ ] **Step 3: Implement** the snap in `survives`.
- [ ] **Step 4: Run** `cargo test -p ofs-radio -p ofs-sim --locked`. If a golden moves (`elrs.*.digests`, `radio-flight.*.digests`), regenerate it as in the Global Constraints, in a **separate commit** `test(radio): regenerate the goldens for the burst-edge fix (#13)`, with the changed digests quoted in the message. If none moves, say so in the ledger.
- [ ] **Step 5: Commit** `fix(radio): a packet on a burst-window edge belongs to the window that starts there (#13)`.

---

### Task 7: The 2.4 GHz 50 Hz sensitivity is -115 dBm (#12)

**Files:**
- Modify: `crates/ofs-radio/src/band.rs:19-21`, `docs/research/elrs-link.md:63`, `docs/roadmap.md:113`, `crates/ofs-radio/tests/elrs.rs:67`
- Regenerate: the four golden files (both OSes, own commit)

- [ ] **Step 1: Update the test first**: `crates/ofs-radio/tests/elrs.rs:67` asserts `sensitivity_dbm(50) == Some(-115.0)`; add a `band.rs` test `the_2g4_modes_match_expresslrs_published_table` asserting all four rows `(50, -115.0), (150, -112.0), (250, -108.0), (500, -105.0)`.
- [ ] **Step 2: Run** `cargo test -p ofs-radio` — Expected: FAIL on the 50 Hz row.
- [ ] **Step 3: Implement**: `G2_4_MODES` 50 Hz -> `-115.0`; fix the doc comment above it (drop "this model has used -117 ... a roadmap row"); `docs/research/elrs-link.md:63` row becomes `-115 dBm | ExpressLRS signal-health table (https://expresslrs.org/info/signal-health/, ELRS 3.x/4.x), checked 2026-10-17`; delete the roadmap bullet.
- [ ] **Step 4: Run** `cargo test --workspace --locked`; the goldens fail: regenerate all four (Windows locally, Linux in WSL) with `OFS_GOLDEN_WRITE=1`, in a separate commit `test(radio): regenerate the goldens for the 50 Hz sensitivity (#12)` that lists the old and new digests and states that the only model change is -117 -> -115 dBm. Confirm the second run without the env var passes on both OSes.
- [ ] **Step 5: Commit** the code/doc change first (`fix(radio): 2.4 GHz 50 Hz sensitivity is -115 dBm, ExpressLRS's published figure (#12)`), then the goldens.

---

### Task 8: A client that hangs up right after `MSP_REBOOT` no longer wedges the reboot (#57)

**Files:**
- Modify: `third_party/betaflight/ofs-sitl.patch` (the `tcpDataOut` hunk, patch lines ~193-204), `docs/roadmap.md:66-68`, `docs/superpowers/history/closeout-evidence.md`
- Test: `crates/ofs-sim/tests/sitl_live.rs`

- [ ] **Step 1: Write the failing live test** `a_client_that_hangs_up_right_after_msp_reboot_does_not_stop_the_reboot` next to `a_betaflight_reboot_is_a_firmware_restart_not_a_crash`: same rig, but `client.send(MSP_REBOOT, &[]).unwrap(); drop(client);` immediately (no waiting), then run until `v.state().fc_restarts == 1` within 5 simulated seconds, run 2 more seconds, and `msp().request(MSP_API_VERSION, ...)` answers. Run it **against the current binary** to watch it fail (`no motor output within 500 ms` / a `FirmwareCrashed` error / `fc_restarts` stays 0).
- [ ] **Step 2: Fix the patch.** In the WSL tree `~/ofs/betaflight` (pinned tag + the current patch, built by `scripts/build-sitl.sh`), edit `src/main/drivers/serial_tcp.c`: in `tcpDataOut`, where it says `if (s->conn == NULL) return;` first discard the pending TX bytes under `s->txLock` (`s->port.txBufferTail = s->port.txBufferHead;`) so `isTcpTransmitBufferEmpty` becomes true and `waitForSerialPortToFinishTransmitting` returns; keep the lockstep capture branch above it untouched. Regenerate the patch with `git diff` against the tag (compare with the old patch: only that hunk, and the hunk header counts, may differ), then verify `git apply --check` on a clean checkout of the pinned tag.
- [ ] **Step 3: Rebuild SITL**: `bash scripts/build-sitl.sh` in WSL (several minutes; the script resets the tree and re-applies the patch, so it proves the patch file, not the working tree). Expected: ends with the `ls -l obj/main/betaflight_SITL.elf` line.
- [ ] **Step 4: Run** the new test 5 times, the whole `sitl_live` file once, and `cargo test -p ofs-fc --test sitl_live -- --ignored --test-threads=1` — Expected: PASS. If the root cause in design call 3 is wrong (the new test still fails after the patch), time-box one hour of investigation with the SITL log (`firmware/sitl.log`), then stop: ledger `Ruling: #57 not fixed` with the evidence added to `closeout-evidence.md`, keep the failing test as `#[ignore = "#57"]`, and leave the issue open with a comment.
- [ ] **Step 5: Docs**: replace the roadmap bullet (`docs/roadmap.md:66-68`) with a one-line note that a hang-up after `MSP_REBOOT` is handled, and add a "fixed" paragraph to the evidence section.
- [ ] **Step 6: Commit** `fix(sitl): drop the TX bytes of a port with no client so a hung-up MSP_REBOOT can finish (#57)`.

---

### Task 9: Close #14 with evidence and bring the docs in line

**Files:**
- Modify: `docs/roadmap.md:71-77` (the port-bind paragraph), `CHANGELOG.md` (an `Unreleased — M7a` section covering every task: `set_sticks` is fallible, the recorder, the events, the Blackbox disarm, the recording finish, the burst edge, -115 dBm and its golden change, the SITL patch)

- [ ] **Step 1: Verify** the patch carries both fixes: `grep -n "BIND_ATTEMPTS\|tcpDyadLock" third_party/betaflight/ofs-sitl.patch` shows the retry loop and the lock (commits `ac97cad`, `6ccbaef`); `gh run list --repo hugowis/open-fpv-sim --workflow ci --limit 15 --json conclusion,databaseId` and `gh run view <id> --log | grep "failed!!"` on the five newest `sitl` jobs show no bind failure since.
- [ ] **Step 2: Rewrite** the roadmap paragraph: the flake had two causes (a racing `dyad_update`, a single-attempt bind), both fixed in the patch, no recurrence in the checked runs (name them).
- [ ] **Step 3: Write the CHANGELOG section.**
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: PASS.
- [ ] **Step 5: Commit** `docs: M7a changelog; the port-bind flake is fixed in the patch (#14)`. After the PR merges, close #14 with `gh issue close 14 --comment "..."` naming `ac97cad`, `6ccbaef` and the runs checked, and put one `Closes #n` per issue in the PR body (GitHub closes only the first it parses otherwise): #54, #55, #58, #57 (unless left open by task 8), #59, #13, #12, #14.

---

## Self-Review

- **Coverage:** #54 -> tasks 1-2 (all four bullets); #55 -> 3; #58 -> 4; #59 -> 5; #13 -> 6; #12 -> 7; #57 -> 8; #14 -> 9. The CHANGELOG/roadmap edits are in 4, 7, 8, 9.
- **Placeholders:** none; tasks 5 and 8 contain a decision point each (the mcap spike, the SITL root cause) with a stated default and fallback.
- **Interfaces:** `Sticks::clamped` and the fallible `set_sticks` (task 1) are consumed by task 2's tests only through `Vehicle`; `disarm_for_shutdown` (task 4) is used only by `Session::drop`; nothing else crosses tasks.
