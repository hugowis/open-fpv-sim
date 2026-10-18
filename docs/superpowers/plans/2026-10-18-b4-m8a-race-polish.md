# B4: M8a Race Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native run, chosen by the user). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M8a deferred minors: the HUD never shows a bogus minute, race events keep their time order inside a batch, the `Session::changes()` race path gains its integration test, the doc nits go away, and the M8a plan's ledger records the opening-edge ruling.

**Architecture:** Four tasks, each one commit. Task 1 is Godot (HUD + its test); task 2 is the Rust event ordering with a unit test; task 3 is the integration test; task 4 is docs.

**Tech Stack:** Godot 4 (GDScript, `scripts/run-godot-tests.sh`), Rust (workspace, MSRV 1.85).

**Spec:** none. The scope is the "Deferred minors" list in the body of PR #84 (`gh pr view 84`), minus the documented known limits (the two-gate facing course, one-gate-per-step) which stay documented in `docs/race-timing.md`.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green.
- `scripts/run-godot-tests.sh` green (Godot headless; the dev-setup section names the binary — if the local Godot is missing, run the suite in CI and say so in the PR body).
- The radio goldens must not change; no model or physics change (ruling: the crossing logic itself is not touched — the one-gate-per-step and facing-gates limits are documented behaviour).
- Ruling (ordering): events are time-ordered **within one `changes()` batch** by a stable sort; cross-batch ordering is the stream's own and is not promised.

## Review Focus

- A lap of 59.95–60.0 s shows `1:00.0`, never `0:60.0` (task 1).
- A race event whose crossing time precedes a state event in the same batch is delivered before it (task 2).
- The `changes()` race event carries the same kind/text the state message's formatter produces (task 3 pins the two together).

---

### Task 1: The HUD rounds before it formats

**Files:**
- Modify: `godot/ui/hud.gd:293` (the lap-time `"%d:%04.1f"` format)
- Test: `godot/tests/test_hud.gd` (the file has lap-line cases — find them with `grep -n "LAP" godot/tests/test_hud.gd`)

**Interfaces:** none (display only). The fix: `snapped(lap_s, 0.1)` first, then derive minutes and seconds from the rounded value, so 59.96 becomes 1:00.0. Same for the LAST/BEST values at lines 294–295 (`%.1f` alone is safe — they carry no minute split; touch only the lap clock).

- [ ] **Step 1: Write the failing Godot test**: a state whose `race_lap_s` is 59.96 formats as `LAP 1   NEXT START   1:00.0` (today: `0:60.0`). Also pin 59.94 → `0:59.9`.
- [ ] **Step 2: Run** `scripts/run-godot-tests.sh` — Expected: the new case FAILS.
- [ ] **Step 3: Implement** the snap-then-format.
- [ ] **Step 4: Run** the Godot suite — Expected: pass.
- [ ] **Step 5: Commit** `fix(godot): the HUD lap clock rounds before it splits minutes`.

---

### Task 2: Race events keep their place in the batch's time order

**Files:**
- Modify: `crates/ofs-sim/src/session.rs` (the race-event append at lines 336–339)
- Test: `crates/ofs-sim/src/session.rs`'s test module (it exists; `race_event`/`race_msg` unit tests live there, lines 565+)

**Interfaces:**
- Produces: `pub(crate) fn events_in_time_order(mut out: Vec<pb::Event>) -> Vec<pb::Event>` — a stable sort by each event's `time_s`; every `changes()` batch passes through it just before returning.

- [ ] **Step 1: Write the failing unit test**: build a batch of two state-derived events at t=1.0, 3.0 and a race event at t=2.0 appended last (exactly today's shape); assert the order is 1.0, 2.0, 3.0 and that a tie keeps insertion order (two events at the same time_s).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib` — Expected: FAIL (no such function).
- [ ] **Step 3: Implement** the sort; call it at the end of the batch assembly in `changes()` (the single place race events are appended, lines 336–339).
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass (the Python circuit's determinism test compares event tuples it collects in order; a re-order inside one batch is exactly the fix it wants).
- [ ] **Step 5: Run** `PYTHONPATH=python python -m pytest python/tests/test_circuit.py -q` — Expected: pass.
- [ ] **Step 6: Commit** `fix(sim): race events are time-ordered within a changes batch`.

---

### Task 3: The integration test through `Session::changes()`

**Files:**
- Modify: `crates/ofs-sim/tests/gates.rs` (the file's existing harness drives gates; find its Session helper with `grep -n "Session\|changes" crates/ofs-sim/tests/gates.rs | head`)

**Interfaces:**
- Consumes: the harness's world-with-gates load, a steered straight flight, `session.changes()`.

- [ ] **Step 1: Write `a_gate_event_arrives_through_changes`**: load the gates world (the file's existing setup), then loop `set_sticks` + `changes()` instead of the state path the other tests use; assert that a `GatePassed`-kind event arrives with the same gate name the state-message tests assert, and that the batch containing it is time-ordered (task 2's guarantee, asserted here end-to-end).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test gates` — Expected: the new test passes; red proof: temporarily skip the task-2 sort and see the batch-order assertion fail only if the harness can produce an out-of-order batch — if it cannot, drop the order assertion and keep the event-delivery one (say so in the commit message).
- [ ] **Step 3: Commit** `test(sim): a gate event reaches the client through Session::changes`.

---

### Task 4: The doc nits and the M8a ledger

**Files:**
- Modify: `godot/world/world.gd:4` — the header's "Nothing here collides with" is stale since M3c; reword to what the world server actually does (it carries the boxes/cylinders the physics collides with and the video link occludes).
- Modify: `python/README.md` — add `gate_count` to the state/race fields it documents (find the race rows: `grep -n "race\\|lap" python/README.md | head`).
- Modify: `docs/race-timing.md` — the Python snippet that prints events: read it, and either fix the snippet to steer a flight or say plainly that it prints nothing on an unsteered flight (whichever is one line).
- Modify: `docs/superpowers/plans/2026-10-17-m8a-gates-and-laps.md` — append a `## Ledger` line: the Review Focus sentence saying the exact opening edge is not counted is wrong; the code and Design call 2 count it (ruled 2026-10-18, PR #84's deferred list).

- [ ] **Step 1: Make the four edits.**
- [ ] **Step 2: Run** `cargo test --workspace --locked` and `python -m pytest python/tests/test_race*.py -q` (or the file `race-timing.md`'s snippet names) — Expected: pass.
- [ ] **Step 3: Commit** `docs: the world header collides, gate_count is documented, race-timing's snippet is honest, the M8a ledger rules the opening edge`.

---

## Not in this plan

- The two-gate facing course and the one-gate-per-step limit: documented in `docs/race-timing.md`, both need real course design to fix (not a minor).
- The visual HUD check in a window: needs a display; the maintainer's manual pass covers it.
- Godot gaining race-course editing: feature work.

## Self-Review

- **Coverage:** PR #84's seven bullets — HUD (1), ordering (2), missing integration test (3), doc nits (4: world.gd, python/README, race-timing.md), plan/code discrepancy (4's ledger line). "At most one gate per step" and the facing-gates laps stay documented (ruled above). The verification gaps (no visual check; e2e not run) belong to the maintainer's manual pass.
- **Placeholders:** task 1 and 3 leave one grep each; task 4's race-timing decision is a one-liner either way.
- **Interfaces:** `events_in_time_order` is produced in task 2 and used in task 3's assertion; the HUD keeps its state keys.
