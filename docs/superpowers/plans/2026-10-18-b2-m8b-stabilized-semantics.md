# B2: M8b Stabilized-Session Semantics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native run, chosen by the user). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M8b deferred minors that touch what a stabilized session *means*: an old build must refuse its bundle loudly instead of replaying it throttle-only, the bundle must record how the session really flew, a contradictory load override must be refused, and the example, tests and docs tell the truth.

**Architecture:** Five tasks, each one commit. Tasks 1–3 are Rust (repro format, server override, bundle truth); tasks 4–5 are Python/tests/docs.

**Tech Stack:** Rust (workspace, MSRV 1.85), Python `ofs`.

**Spec:** none. The scope is the "Deferred minors" list in the body of PR #85 (`gh pr view 85`).

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows; `PYTHONPATH=python python -m pytest python/tests -q` green.
- The radio goldens must not change. Replay digests of existing tests must not change except where a test pins the bundle's new keys.
- Ruling (format): `BUNDLE_FORMAT` becomes 2; the writer emits 2; the reader accepts **1 and 2** (identical layouts — 2 exists only so a pre-M8b build refuses a stabilized bundle with "this build reads 1" instead of misreading it).
- Ruling (override): a request that would downgrade an `open_loop_stabilized` quad to plain open-loop is `invalid_argument`, not silent.

## Review Focus

- A pre-M8b `ofs-sim --replay` of a stabilized bundle must stop at the version line, never fly (task 1).
- A 0.1.x-era format-1 bundle (plain open-loop) must still replay on this build (task 1).
- `load(open_loop_fc=True)` on the shipped stabilized quad must fail with a message naming the fix, in the gRPC error path and therefore in Python and Godot (task 2).
- A stabilized session started from the quad-file kind alone (Godot's only way) must write `open_loop_fc = true`, `stabilize = true` in its bundle, and its replay must still reproduce (task 3).

---

### Task 1: Bundle format 2 — old builds refuse a stabilized bundle loudly

**Files:**
- Modify: `crates/ofs-sim/src/repro.rs` (`BUNDLE_FORMAT`, line 15; the writer at line 197; the version check at lines 271–274; the unit test asserting `Some(1)` at line 421)
- Modify: `docs/repro-bundle.md` (the format-version sentence — find it with `grep -n "format_version" docs/repro-bundle.md`), `CHANGELOG.md` (a line under the new Unreleased section: old builds refuse M8b+ stabilized bundles)

**Interfaces:**
- Produces: `pub const BUNDLE_FORMAT: u32 = 2;`. `replay`'s check becomes `if format != 1 && format != 2 { Err(...) }` with the message unchanged in shape (`bundle.toml: format_version {n} (this build reads 1 and 2)`).

- [ ] **Step 1: Write the failing test** in `repro.rs`'s test module: `a_format_1_bundle_still_replays` — hand-write a minimal `bundle.toml` with `format_version = 1` and the keys `replay` reads (copy the key list from the reader; values from an existing test bundle), assert the error is **not** the version error (any later error proves the check passed). Update the existing assertion at line 421 to `Some(2)` and the refusal test at line 598 stays (`format_version = 99`).
- [ ] **Step 2: Run** `cargo test -p ofs-sim --lib repro` — Expected: the new test FAILS (format 1 is rejected today) and the 421 assertion fails.
- [ ] **Step 3: Implement** the constant, the acceptance of 1 and 2, and the message; fix the doc sentence.
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass (every bundle any test writes now says 2 and every reader accepts it).
- [ ] **Step 5: Commit** `fix(sim): stabilized bundles are format 2, so an older build refuses them instead of replaying throttle-only`.

---

### Task 2: Refuse the downgrade override

**Files:**
- Modify: `crates/ofs-sim/src/server.rs` (the `fc_override` block at lines 271–274)
- Test: the gRPC test file that covers load errors (find it: `grep -rn "stabilize needs open_loop_fc" crates/ --include="*.rs"` — its neighbours are the pattern)

**Interfaces:**
- Produces: no signature change. `LoadRequest { open_loop_fc: true, stabilize: false }` (or `stabilize` unset) against a quad whose file kind is `OpenLoopStabilized` fails `invalid_argument` with: this quad file is `open_loop_stabilized`; load with `stabilize = true` or without the `open_loop_fc` override.

- [ ] **Step 1: Write the failing test** beside the existing override tests: the shipped stabilized quad (find its path: `grep -rln "open_loop_stabilized" quads/`) loaded with `open_loop_fc: true, stabilize: false` is an invalid-argument error whose message names the kind; the same request with `stabilize: true` loads.
- [ ] **Step 2: Run** that test file — Expected: FAIL (today it loads throttle-only).
- [ ] **Step 3: Implement**: when `fc_override == Some(FcKind::OpenLoop)` and the loaded config's fc kind is `OpenLoopStabilized`, return the error before building (the config is already loaded at that point, lines 260–266).
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass.
- [ ] **Step 5: Commit** `fix(sim): loading a stabilized quad with a plain open-loop override is refused, not silently throttle-only`.

---

### Task 3: The bundle records how the session really flew

**Files:**
- Modify: `crates/ofs-sim/src/server.rs` (the `BundleSource::new` call at lines 282–296 and the `r.stabilize` patch-up at line 295)
- Modify: `crates/ofs-sim/src/repro.rs` (a pure helper + unit tests)
- Test: `crates/ofs-sim/tests/repro.rs` or the gRPC test file (wherever a stabilized session is failed into a bundle today — find with `grep -rn "stabiliz" crates/ofs-sim/tests/repro.rs crates/ofs-sim/tests/grpc.rs`)

**Interfaces:**
- Produces: `pub(crate) fn effective_fc_flags(file_kind: FcKind, requested: Option<FcKind>) -> (bool, bool)` in `repro.rs` — `(open_loop_fc, stabilize)` of what the vehicle actually flies: `requested.unwrap_or(file_kind)`, then `(kind is OpenLoop | OpenLoopStabilized, kind == OpenLoopStabilized)`.

- [ ] **Step 1: Write the failing tests**: unit tests over the matrix (file `OpenLoopStabilized`, no request → `(true, true)`; file `OpenLoopStabilized`, request `OpenLoopStabilized` → `(true, true)`; file `OpenLoop`, request `OpenLoop` → `(true, false)`; file `Sitl`/`None` → `(false, false)`); one integration assertion wherever a stabilized session already produces a bundle: its `bundle.toml` has `open_loop_fc = true` and `stabilize = true`.
- [ ] **Step 2: Run** — Expected: FAIL (today a quad-file-kind stabilized session records `(false, false)` — the server passes `req.open_loop_fc`/`req.stabilize`, both false when the kind came from the file).
- [ ] **Step 3: Implement** the helper; in the session's blocking closure, after `vehicle::build` succeeds, set `repro`'s two flags from the helper using the config's kind and the request's override (the pieces are already in scope there). The writer itself is unchanged.
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass; replay of a stabilized bundle still reproduces (the override now matches the file kind, which replay already applied via the bundled quad).
- [ ] **Step 5: Commit** `fix(sim): a repro bundle records the session's effective FC kind, not the request flags`.

---

### Task 4: `fly_circuit` tells the truth about its session

**Files:**
- Modify: `python/examples/circuit.py` (the `fly_circuit` docstring at line 79; the `angle_probe` comment at line 31)
- Modify: `python/tests/test_sitl_circuit.py` (the warm-up comment at line 26 — re-read `test_sitl_hover.py` first and state what actually happens)

- [ ] **Step 1: Docstring** gains one sentence: the session must be loaded to match (`open_loop_fc=not betaflight, stabilize=not betaflight`, the circuit world, `seed=1`); a mismatch shows up as waypoint timeouts, not an error.
- [ ] **Step 2: Comment at line 31**: drop the phantom `angle_probe` script name; keep the measurement ("a 0.3 stick leans about 7° on the shipped Betaflight tune — hold the stick for 2 s and read the attitude") and put that recipe in `docs/scripted-flight.md` where the gains are discussed.
- [ ] **Step 3: Fix the warm-up comment** in `test_sitl_circuit.py` to say what the code does (verify against `test_sitl_hover.py` first).
- [ ] **Step 4: Run** `python -m pytest python/tests/test_circuit.py -q` (no SITL needed) — Expected: pass.
- [ ] **Step 5: Commit** `docs(python): fly_circuit's session requirements and the lean measurement stand on their own`.

---

### Task 5: Circuit test margins, the Godot coverage line, the plan ledger

**Files:**
- Modify: `python/tests/test_circuit.py` (`test_an_unreachable_waypoint_raises_naming_it` at line 54; add a `max_seconds` test)
- Modify: `docs/scripted-flight.md` (the "Not covered" sentence at line 88)
- Modify: `docs/superpowers/plans/2026-10-17-m8b-scripted-flight.md` (append a `## Ledger` section)

- [ ] **Step 1: Widen the margin**: `per_waypoint_s=5.0` → `8.0` in the unreachable-waypoint test (the first waypoint is 20 m out; 5 s left about 0.4 s of margin on a slow CI runner).
- [ ] **Step 2: Write `test_max_seconds_raises`**: `fly_circuit(sim, max_seconds=0.1)` raises `RuntimeError` matching `more than 0 s` (the message formats `{max_seconds:.0f}`).
- [ ] **Step 3: Run** `python -m pytest python/tests/test_circuit.py -q` — Expected: the new test passes; the widened one still passes and still names waypoint 1 at 10000.
- [ ] **Step 4: Fix the Godot line** at `docs/scripted-flight.md:88`: the game has no `stabilize` option, but a quad file with `kind = "open_loop_stabilized"` flies stabilized in Godot — covered by the Rust gRPC stabilizer test, not by a Godot-specific one.
- [ ] **Step 5: Append the ledger**: the three unrecorded deviations from the PR #85 review (`fly_circuit` returns `final` rather than `states`; the gRPC test's rename to `a_stabilized_session_obeys_a_roll_stick_and_a_plain_open_loop_one_ignores_it`; the Python stabilize test checks the roll, the Rust test checks level holding) — one line each, marked `recorded 2026-10-18`.
- [ ] **Step 6: Commit** `test: the circuit's error branches are pinned; the M8b ledger records its deviations`.

---

## Not in this plan

- A `LoadReply` field exposing the FC kind (would let `fly_circuit` validate against the session): a protocol change, not a minor.
- Godot gaining a `stabilize` option: feature work, tracked by the scripted-flight axis.

## Self-Review

- **Coverage:** PR #85's list — bundle format (1), bundle flags (3), silent override (2), `fly_circuit` check (4, as far as a minor can: docstring + example pairing; the protocol-level check is ruled out above), stale comments (4), test margins (5), unrecorded deviations (5), Godot docs line (5).
- **Placeholders:** task 2 and 3 leave one file-lookup each (the quad path, the existing bundle-producing test) with the grep that finds it; everything else carries exact sites.
- **Interfaces:** `BUNDLE_FORMAT` consumers all go through the constant; `effective_fc_flags` is produced in task 3 and used only there.
