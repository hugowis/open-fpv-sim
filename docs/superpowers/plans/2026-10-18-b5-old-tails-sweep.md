# B5: The M5–M7a Small-Tails Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native run, chosen by the user). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the deferred minors left over from M5, M6c, M6d, M6e, M7a and the 0.2.0 release work: one sweep in the M6d style — validate reports every problem it can, the live SITL tests kill only their own slot, the fake-SITL tests cannot leak their child, a redundant test goes away, the Blackbox "last second" is really asserted, a Load/Unload mid-recording says so, and the release checker catches what it said it could not.

**Architecture:** Seven tasks, each one commit. Tasks 1–6 are Rust/test changes; task 7 is Python/docs. The live tests (tasks 3 and 5) run in WSL locally and in CI's `sitl` job.

**Tech Stack:** Rust (workspace, MSRV 1.85), Python, Betaflight SITL (WSL) for tasks 3 and 5.

**Spec:** none. The scope is the still-open deferred minors of PRs #80, #81, #82, #83 and #87 (their bodies' "Deferred minors" sections), as inventoried on 2026-10-18.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `PYTHONPATH=python python -m pytest python/tests -q` green.
- The radio goldens must not change; `cargo fmt` is not a gate.
- Ruling (pkill): the live tests kill by their session's slot (`--port-offset`), derived from the session's `configurator_address`, using the existing `net::SLOT_KILL_SCRIPT` — never `pkill -x` by process name.
- Ruling (recording event): a Load/Unload that ends a session with a running MCAP recording publishes `RecordingStopped` with a text naming the file and the cause — mirroring how `InputsRecordingStopped` is published on the same path (find it: `grep -n "InputsRecordingStopped" crates/ofs-sim/src/server.rs`).
- Ruling (redundant test): `a_replayed_journal_sees_the_clamped_sticks` is deleted — the stick clamp is pinned by `set_sticks_clamps_what_it_accepts` (verify with `grep -rn "set_sticks_clamps" crates/ --include="*.rs"` before deleting, and name it in the commit message).
- Ruling (submission errors): a TOML *syntax* error stays a single stop-everything problem; every *schema* problem (unknown key, wrong type) is reported in one pass. The line-number prefix from M6d is kept.

## Review Focus

- A submission.toml with three wrong keys lists three problems, not one (task 1).
- A developer running the live SITL tests beside their own SITL loses nothing (task 3's scoped kill).
- A victim test whose own assertion fires leaves no `betaflight_SITL`-lookalike process behind (task 4's guard).
- A recording running when the client Loads a new quad produces a `RecordingStopped` event naming the old file before the new session starts (task 5).

---

### Task 1: `ofs-validate` reports every schema problem in one pass

**Files:**
- Modify: `crates/ofs-validate/src/submission.rs` (`load` at line 136; the `Raw` struct above it)
- Test: `crates/ofs-validate/tests/submission.rs`

**Interfaces:**
- Produces: `load` keeps its signature (`Result<Submission, Vec<String>>`). The parse becomes two-step: `toml::from_str::<toml::Table>` (syntax errors stop here, reported with the existing line-number helper), then the `Raw` fields are extracted per key from the table (each known key deserialized on its own; each failure is one problem naming the key), and every table key not in `Raw` is one `unknown key` problem. The existing semantic checks below are unchanged.

- [ ] **Step 1: Write the failing test**: a `submission.toml` with an unknown key `colour = 1`, a wrong-typed `title = 5`, and a valid `kind` — `load` returns problems naming all three: `colour` unknown, `title` wrong type, and no complaint about `kind`.
- [ ] **Step 2: Run** `cargo test -p ofs-validate --test submission` — Expected: FAIL (only the first of the two problems comes back).
- [ ] **Step 3: Implement** the two-step parse and per-key extraction (the `Raw` field list is the known-key list; `[origin]`'s keys are checked the same way when `origin` is a table).
- [ ] **Step 4: Run** `cargo test -p ofs-validate` — Expected: pass (the existing line-number and semantic tests keep passing).
- [ ] **Step 5: Commit** `fix(validate): submission.toml reports every schema problem, not the first`.

---

### Task 2: The release checker guards what it said it could not

**Files:**
- Modify: `python/tests/test_release.py` (read it first; its CHANGELOG checks know the section list)
- Modify: `docs/dev-setup.md:214–216` — the protocol-12 line lists `LoadRequest.stabilize` alongside the other protocol-12 additions

**Interfaces:** none.

- [ ] **Step 1: Write the two failing checks** (as pytest tests over a synthetic changelog string, following the file's existing style): an `## Unreleased` heading followed directly by another `##` heading (no `### Added/Fixed/Changed` between) is an error; a `###` section appearing before any `##` heading is an error.
- [ ] **Step 2: Run** `python -m pytest python/tests/test_release.py -q` — Expected: the new tests FAIL (today both pass silently).
- [ ] **Step 3: Implement** the two checks in the script's heading walk.
- [ ] **Step 4: Fix the dev-setup protocol line** (add `stabilize` with the other M8b additions).
- [ ] **Step 5: Commit** `test(release): the checker catches an empty Unreleased section and a misplaced heading`.

---

### Task 3: The live SITL-kill test kills only its own slot

**Files:**
- Modify: `crates/ofs-sim/tests/sitl_live.rs` (the `pkill -9 -x betaflight_SITL` at line ~305 and its mitigating comment at ~313)

**Interfaces:**
- Consumes: the session's `configurator_address` (`tcp://127.0.0.1:5761 + 10·slot` → `slot = (msp − 5761) / 10`); `ofs_fc::sitl::net::SLOT_KILL_SCRIPT` (pub; `sh -c <script> sh <offset>`).

- [ ] **Step 1: Replace the pkill**: after load, derive the slot from the address; kill with `Command::new("sh").args(["-c", SLOT_KILL_SCRIPT, "sh", offset.to_string()])` followed by the same SIGKILL escalation the test already does if the process lingers (keep the test's existing wait-for-exit logic). Delete the "takes down every Betaflight SITL" comment and replace it with the truth ("kills only this session's slot's SITL").
- [ ] **Step 2: Run in WSL** (PowerShell): `cargo test -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1` — Expected: pass. Red proof: start a second fake `betaflight_SITL`-named sleeper outside the slot before the test and assert it still lives afterwards (temporary second test, or fold into this one: spawn `sleep 60` named via `exec -a betaflight_SITL`, run the test, assert the sleeper survived — then drop the sleeper part if it over-complicates the test, keeping at least one manual run that shows it).
- [ ] **Step 3: Commit** `test(sim): the live SITL-kill kills its slot's processes, not every SITL on the machine`.

---

### Task 4: The victim tests cannot leak their fake SITL

**Files:**
- Modify: `crates/ofs-fc/tests/sitl_errors.rs` (the fake-SITL `Child`/process pattern; read the file first — the leaks happen when an assertion fires between spawn and the test's own kill)

**Interfaces:**
- Produces: a small guard struct in the test file (`struct KillOnDrop(Child)` with `impl Drop` = kill + wait) wrapping every spawned fake SITL, replacing the manual end-of-test kills.

- [ ] **Step 1: Refactor** the spawns to the guard (mechanical; keep every assertion as it is).
- [ ] **Step 2: Prove it**: pick one test, insert a deliberate `panic!("probe")` right after the spawn, run it, and confirm no process survives (a second run of the whole file must not flake on ports); remove the probe.
- [ ] **Step 3: Run** `cargo test -p ofs-fc --test sitl_errors` — Expected: pass, twice in a row.
- [ ] **Step 4: Commit** `test(fc): the victim tests kill their fake SITL even when they fail`.

---

### Task 5: The Blackbox "last second" is content, not just an end marker

**Files:**
- Modify: `crates/ofs-sim/tests/sitl_live.rs` (the End-of-log test at lines 231–255)
- Read first: `crates/ofs-sim/src/blackbox.rs` — what the frames carry and what constants the writer uses (the test decodes with the same constants; no new public API unless the frame timing is unavailable, in which case stop and rule in the PR body)

**Interfaces:**
- Produces: the finished-log assertion gains a content check: decoding the log's frame loop times (the writer's own field/frame format) shows logged time within the final second of the flight — i.e. `max(log_time)` is within 1 s of the session's end time.

- [ ] **Step 1: Write the failing assertion** in the existing test (decode the finished log's frame times; assert the last one is ≥ end−1 s). Red proof: temporarily make the writer cut the last second (a local patch you revert) and see the assertion fail; revert.
- [ ] **Step 2: Run in WSL**: the sitl_live file as in task 3 — Expected: pass.
- [ ] **Step 3: Commit** `test(sim): a finished Blackbox log's last frame is within a second of the flight's end (#58)`.

---

### Task 6: A Load/Unload mid-recording publishes `RecordingStopped`

**Files:**
- Modify: `crates/ofs-sim/src/server.rs` (where `end_session` tears the previous session down; mirror the `InputsRecordingStopped` mechanism found via the Global Constraints grep)
- Test: `crates/ofs-sim/tests/grpc.rs` (its start-recording fixtures)

**Interfaces:** none (event kind and text only). Text: `recording stopped: the session ended (<Load|Unload>)`, naming the recording's path.

- [ ] **Step 1: Write the failing test**: start a recording on a session, issue a second `Load` (or an `Unload` if the protocol has one), assert the *new* session's first `changes()` batch (or the failure path the protocol provides — mirror exactly how the inputs-recording event reaches the client) carries `RecordingStopped` naming the file.
- [ ] **Step 2: Run** — Expected: FAIL (no such event today).
- [ ] **Step 3: Implement** on the end-of-session path: if a recording is active, push the event through the same channel the inputs event uses (before the session drops, so it is queued for delivery).
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass.
- [ ] **Step 5: Commit** `fix(sim): ending a session stops its recording with a named event, like inputs recordings`.

---

### Task 7: The redundant journal test goes away

**Files:**
- Modify: `crates/ofs-sim/tests/journal.rs` (delete `a_replayed_journal_sees_the_clamped_sticks`)

- [ ] **Step 1: Verify the clamp is pinned** (`grep -rn "set_sticks_clamps" crates/ --include="*.rs"` — expect `set_sticks_clamps_what_it_accepts` in the grpc/session tests); name the pinning test in the commit message.
- [ ] **Step 2: Delete the test; run** `cargo test -p ofs-sim` — Expected: pass.
- [ ] **Step 3: Commit** `test: the replayed-journal sticks test is redundant; the clamp is pinned at set_sticks`.

---

### Task 8: The sweep's changelog entries

**Files:**
- Modify: `CHANGELOG.md` (the Unreleased section the earlier batches created): validate's multi-error reporting, the recording-stopped-on-session-end event, the scoped live-test kill, the release checker's new guards.

- [ ] **Step 1: Add the entries; run** `python -m pytest python/tests/test_release.py -q` — Expected: pass (the new guards included).
- [ ] **Step 2: Commit** `docs: the sweep's changelog entries`.

---

## Not in this plan

- The capacity-fade float-fuzziness and `Battery::new` positive-factor asserts: parked by the M6d ruling ("revisit only with a real report").
- The four `Co-Authored-By` trailer commits: history, won't-fix.
- The stale table in `docs/superpowers/specs/2026-10-11-m4-sandbox-design.md`: a history document, won't-fix.
- The `ofs-fc` victim test's `sh`-on-PATH requirement on Windows: pre-existing, documented ("as before").

## Self-Review

- **Coverage:** #80's multi-error item (1); #87's checker gaps and protocol line (2); #81's pkill blast radius (3); #82's victim-test leak (4); #83's #58 acceptance gap (5) and the Load/Unload recording_stopped (6) and the redundant test (7); changelog (8). #81's trailer commits and #82's history-doc table are ruled out above.
- **Placeholders:** tasks 3, 5 and 6 leave one read-first each with the rule; task 4's guard shape is given.
- **Interfaces:** `load` keeps its signature; `SLOT_KILL_SCRIPT` already pub; no new public Rust API except where task 5 finds the frame constants unreachable (stop-and-rule case).
