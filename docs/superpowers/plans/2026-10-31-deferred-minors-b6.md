# B6: The 2026-10-31 Deferred-Minors Sweep Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the deferred minors left over from the PRs merged between 2026-10-19 and 2026-10-31 (#100–#162): promote the `windows-sitl` CI job to gating, make a failed DVR export retryable and panic-safe, give the game's launcher an automatic port, make the terrain slope cap cross-OS exact, record radio goldens for the three new quads, and name the evidence server build in the mid-write doc.

**Architecture:** Six tasks, each one commit. Tasks 2–4 are Rust changes in the M6d style (failing test first); task 1 is CI/docs; tasks 5–6 are recorded data and doc truth. The bigger findings of the same sweep are issues, not this plan: #163 (the manual by-hand checklist), #164 (`battery_current_a` as a clamp), #165 (the measured magnetometer into SITL).

**Tech Stack:** Rust (workspace, MSRV 1.85), GitHub Actions (`ci.yml`, `macos-survey.yml`), Betaflight SITL (WSL) only where a golden is recorded.

**Spec:** none. The scope is the still-open deferred minors of PRs #110, #119, #137, #140 and #161 (their bodies' "deferred"/"known limits"/"not done" sections), as inventoried on 2026-10-31. Already consumed elsewhere: #137's letterbox/clean-feed-first-field limits and `fit_image`'s per-field copy are documented limits (`docs/roadmap.md` §Video); #144's contact limits are documented (§Physics); the WSL/Windows lock-domain split is documented (§Flight controller and SITL); #119's reload/lost-session DVR toast and F1 help line were rebuilt by #137/#149 (verified on main 2026-10-31); #149's non-goals are recorded on #135; every "check by hand" item is #163.

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows and in WSL; `PYTHONPATH=python python -m pytest python/tests -q` green (unchanged by every task here).
- The radio goldens of `freestyle` and `weak-link` must not change; task 5 only adds lines (and the macOS files grow the same lines).
- `actionlint` on every workflow edit; `cargo fmt` is not a gate.
- Ruling (never overwrite): an export still never overwrites a file it did not just create itself; a *failed* ffmpeg's own partial `.mp4` is removed so the name is free again (task 2).
- Ruling (port 0): a `server_addr` whose port is 0 means "the OS picks" — the launcher spawns with `--listen <host>:0 --port-file <file>` and connects to the address the server wrote, exactly as Python's `ofs.launch()` has since #110. No new setting key (task 3).
- Ruling (slope cap): the 60° decision is `slope * slope <= 3.0` (tan²60° = 3 exactly; multiply is exact-rounded, unlike libm `atan`) — the degrees value stays only in the error message (task 4).
- CHANGELOG "Unreleased" gets one line per code task; doc-only edits (tasks 1, 6) do not.

## Review Focus

- A user whose `ffmpeg` dies mid-encode retries the export and it works — no "already exists" from the corpse of the first attempt (task 2).
- A panicking export thread surfaces as a polled error, not silence (task 2).
- Two game instances on one machine both start and fly, with `50051` held by a stale process (task 3's `127.0.0.1:0`).
- A world file with a slope within an ulp of 60° validates identically on Windows, Linux and macOS (task 4).
- CI's macOS job still passes after task 5 — the macOS golden files carry the three new quads too, or the job fails on the next run (task 5).

---

### Task 1: Promote `windows-sitl` to a gating job

The job's own comment sets the criterion: "promote it after about ten green runs" (`.github/workflows/ci.yml:176-177`). The record: every `ci` run on `main` since the job landed (2026-10-25) is green for `windows-sitl` — 9 runs through 41209520491, plus the 4 green branch runs before that. The macOS job stays `continue-on-error` (8 of 9, the failure being the pytest orphan-SITL flake recorded on #118).

**Files:**
- Modify: `.github/workflows/ci.yml:176-180` (the comment and `continue-on-error: true`) and `:238` (`publish`'s `needs`)
- Modify: `docs/roadmap.md` — the "SITL runs natively on Windows" paragraph (lines 69-73), which is stale: it says "no CI job builds it", written before #145
- Modify: `docs/research/sitl-native-windows.md` — one line noting the promotion and the green-run count

**Interfaces:** none.

- [ ] **Step 1:** In `ci.yml`, delete the `continue-on-error: true` line under `windows-sitl:` and rewrite the comment above it to say the job gates (promoted 2026-10-31 after 13 consecutive green runs) and that the macOS job is the one still waiting on a clean record (#118).
- [ ] **Step 2:** In the `publish:` job's `needs:` (line 238), add `windows-sitl`: `needs: [core, windows, sitl, windows-sitl]`.
- [ ] **Step 3:** Run `actionlint` on the workflow. Expected: clean.
- [ ] **Step 4:** Rewrite the roadmap's native-Windows SITL paragraph from what is true now: the `windows-sitl` CI job builds it (MSYS2 ucrt64, cached, `scripts/build-sitl-windows.sh`), live-tests it from the runner's PowerShell, and gates `publish` since this change; keep whatever of the old paragraph is still true (the stub file, the bit-for-bit comparison caveat) and reconcile the DLL story against `scripts/build-sitl-windows.sh` and #105 before writing it — the old text's "needs two Cygwin DLLs" and #145's "DLL-free build" cannot both be quoted blindly.
- [ ] **Step 5:** Add the promotion line (with the run-count evidence) to the record table in `docs/research/sitl-native-windows.md`.
- [ ] **Step 6:** Commit `ci: windows-sitl gates the publish job after a green record`.

### Task 2: A failed MP4 export retries, and a panicking export reports

Two #137 deferrals in `crates/ofs-client/src/dvr_export.rs`. Today a failing ffmpeg leaves the partial `<stem>.mp4` it was writing, and `export_mp4`'s pre-check (`:175-177`) then refuses every retry with "already exists". And the export thread (`spawn_with`, `:126-138`) has no `catch_unwind`: a panic in it dies silently and the poller never hears an outcome.

**Files:**
- Modify: `crates/ofs-client/src/dvr_export.rs` (`export_mp4` at line 173; `spawn_with` at line 124)
- Test: `crates/ofs-client/tests/dvr_export.rs` (has `fresh_dir`/`write_avi` helpers; no test needs ffmpeg)

**Interfaces:**
- Produces: `export_mp4` keeps its signature and its never-overwrite pre-check; on a failed run it best-effort removes the partial it caused. New `fn catch_panics(avi: PathBuf, job: impl FnOnce() -> Vec<ExportDone>) -> Vec<ExportDone>` converts a panic into one `ExportDone { kind: "export", result: Err(..) }`; `spawn_with` runs its body through it.

- [ ] **Step 1: Write the failing test.** A fake ffmpeg next to nothing real: in `fresh_dir`, write `ffmpeg.exe` (Windows) or `ffmpeg` (unix, `+x`) whose script takes the last argument as its output path, writes `PARTIAL` into it, prints `synthetic ffmpeg failure` to stderr, and exits 1. Assert `export_mp4(&fake, &avi)` is an error carrying `synthetic ffmpeg failure` **and** the partial `.mp4` no longer exists **and** a second `export_mp4(&fake, &avi)` attempt is not refused with "already exists".
- [ ] **Step 2: Run** `cargo test -p ofs-client --test dvr_export` — Expected: FAIL (the partial survives; the retry says "already exists").
- [ ] **Step 3: Implement.** In `export_mp4`'s failure branch (after `:186`): `let _ = std::fs::remove_file(&mp4);` before building the error, and say so in the doc comment ("a failing run removes the partial it wrote, so the name is free for a retry").
- [ ] **Step 4: Run** the test — Expected: pass, including the pre-existing `export_mp4_with_a_missing_program_is_an_error_and_leaves_the_avi` (a missing program creates no partial; nothing to remove).
- [ ] **Step 5: Write the failing test for the panic guard.** Drive `catch_panics(PathBuf::from("a.avi"), || panic!("synthetic boom"))` (in the module's `#[cfg(test)]` block, `mod tests`, next to `last_line_is_the_last_non_blank_line_trimmed`); assert it returns one outcome whose `kind` is `"export"` and whose `Err` mentions the panic. Change `spawn_with`'s thread to `let outcomes = catch_panics(avi.clone(), move || { ..collect into Vec instead of sending.. }); for o in outcomes { tx.send(o).ok(); }`.
- [ ] **Step 6: Run** `cargo test -p ofs-client` — Expected: pass. (`std::panic::catch_unwind` with `AssertUnwindSafe`; the panic hook still prints — acceptable, the thread is a leaf.)
- [ ] **Step 7:** In `docs/roadmap.md` §Video, extend the DVR limits sentence with: an export still running when the game quits is lost (recorded now; unchanged by this task).
- [ ] **Step 8:** CHANGELOG "Unreleased": a failed MP4 export clears its partial file so a retry works, and a panicking export reports instead of going silent.
- [ ] **Step 9:** Commit `fix(dvr): a failed export retries and a panicking one reports`.

### Task 3: The game's launcher takes an automatic port

#110 gave Python's `ofs.launch()` the `--port-file` treatment and left the Godot client's launcher behind: `ServerProcess::spawn` (`crates/ofs-client/src/launch.rs:18`) passes `--listen <addr>` with the fixed `server_addr` (default `127.0.0.1:50051`, `godot/scripts/settings.gd:69`), so any process on that port breaks game launch ("check that its port is free", `app.gd:16`).

**Files:**
- Modify: `crates/ofs-client/src/launch.rs` (`ServerProcess::spawn` gains the port-file mode; a new `wait_for_addr` reader)
- Modify: `crates/ofs-client/src/worker.rs:154` (spawn, then connect to the resolved address when port 0 was asked)
- Test: `crates/ofs-sim/tests/client_launch.rs` (the one suite that runs the real binary; read it first for how it builds `LaunchSpec` and waits)

**Interfaces:**
- Consumes: `ofs-sim --port-file FILE` (exists since #110; the server binds before it writes).
- Produces: `ServerProcess::spawn(spec, listen)` keeps its signature — when `listen`'s port parses as 0, it additionally passes `--port-file <data-dir>/server.port` and returns a process whose `resolved_addr(&self, timeout: Duration) -> Result<String, ClientError>` reads the file (poll at 50 ms, fail with the log tail if the process exits first, mirroring `a_server_that_exits_during_startup_is_reported_with_its_status_and_log` in `crates/ofs-client/tests/launch.rs:29`). `worker.rs` uses `resolved_addr` only in the port-0 case; every other address behaves exactly as today.

- [ ] **Step 1: Write the failing test** in `client_launch.rs`: a `Settings` with `server_addr: "127.0.0.1:0"` and a launch spec for the real binary — start the client, load the shipped quad, assert the session answers and the connected address is loopback with a non-zero port; then a second client started while the first lives does the same (two servers, two OS-picked ports). Then hold `127.0.0.1:50051` with a bound-and-listening socket and repeat: still fine, because port 0 was asked.
- [ ] **Step 2: Run** `cargo test -p ofs-sim --test client_launch` — Expected: FAIL (port 0 is passed to `--listen` verbatim today and the connect address stays `:0`).
- [ ] **Step 3: Implement** the `--port-file` pass-through, the `resolved_addr` reader, and the `worker.rs` branch.
- [ ] **Step 4: Run** `cargo test -p ofs-client -p ofs-sim --test client_launch` — Expected: pass, including the two pre-existing launch tests.
- [ ] **Step 5: Docs.** `README.md`'s settings table row for `server address`: port 0 means the OS picks (the launcher reads back what the server bound); one line in `docs/dev-setup.md` where the fixed port is described.
- [ ] **Step 6:** CHANGELOG "Unreleased": `server_addr` port 0 lets the game start beside anything on the fixed port.
- [ ] **Step 7:** Commit `feat(client): an automatic server port for the game's launcher (port 0 + --port-file)`.

### Task 4: The terrain slope cap is cross-OS exact

#161's deferred minor: the 60° check at `crates/ofs-config/src/world.rs:651` does `ground.max_slope().atan().to_degrees()` and compares degrees; libm `atan` may differ by an ulp between OSes, so a world within an ulp of the cap could validate on one and fail on another.

**Files:**
- Modify: `crates/ofs-config/src/world.rs:645-660` (the `[terrain]` validation around line 651)
- Test: the terrain world tests in `crates/ofs-config/tests/world.rs` (where #161's validation tests live; find them with `grep -n "slope" crates/ofs-config/tests/world.rs`)

**Interfaces:** none — the decision becomes `slope * slope <= 3.0` (60° exactly; `tan²60° = 3`, and a multiply is exact-rounded, unlike `atan`); the error message still names degrees via `atan` (display only).

- [ ] **Step 1: Write the failing test.** A `[terrain]` whose parameters put the steepest grid slope at the largest f64 strictly below `sqrt(3.0)` (compute `f64::from_bits(3.0f64.sqrt().to_bits() - 1)`) validates; one at the smallest value strictly above `sqrt(3.0)` (`to_bits() + 1`) fails with a message naming 60 degrees. Both on this OS — the exactness claim is what makes them OS-independent.
- [ ] **Step 2: Run** `cargo test -p ofs-config --test world` — Expected: FAIL if the current `atan` path disagrees at the boundary (it may pass by luck on this OS; the implementation step is still required, and the test then pins it).
- [ ] **Step 3: Implement** the squared comparison; keep `atan` for the message.
- [ ] **Step 4: Run** `cargo test -p ofs-config` — Expected: pass, `worlds/hills.toml` unchanged (its slopes are far from the cap).
- [ ] **Step 5:** Commit `fix(config): the terrain slope cap decides exactly (slope² vs 3), not through libm atan`.

### Task 5: Radio goldens for the three new quads

#140's deferral: `toothpick`, `race` and `longrange` have no golden digests. The golden test (`crates/ofs-sim/tests/radio_golden.rs:84`) hardcodes `freestyle` and `weak-link` and compares the **whole file** (`assert_eq!(text, expected)` at `:113`), so adding quads means all four OS files grow the same lines — including both macOS files, which only a macOS runner can write (the `macos-survey.yml` workflow has the `write_goldens` input for exactly this, named in the test's own panic message).

**Files:**
- Modify: `crates/ofs-sim/tests/radio_golden.rs:90-99` (the `current` array)
- Modify: `crates/ofs-sim/tests/golden/radio-flight.{linux,windows,macos-aarch64,macos-x86_64}.digests`

**Interfaces:** none — three new `(name, flight(quad, slot, path))` rows: `toothpick` from `opendrone-3t-toothpick.toml`, `race` from `opendrone-5r-race.toml`, `longrange` from `opendrone-7lr-longrange.toml`, each with a fresh slot (1 and 3 are taken; use 4, 5, 6) and the same stick script.

- [ ] **Step 1:** Add the three rows; run `cargo test -p ofs-sim --test radio_golden` — Expected: FAIL on the whole-file compare (missing lines).
- [ ] **Step 2:** Record this OS: `OFS_GOLDEN_WRITE=1 cargo test -p ofs-sim --test radio_golden` (Windows, then WSL for the linux file; the write branch rewrites the whole file, so the freestyle/weak-link lines come back identical — verify with `git diff`: two added lines per file, nothing else. If any old line moved, stop: that is a radio-chain change, not a recording).
- [ ] **Step 3:** Record macOS: trigger `macos-survey.yml` with `write_goldens` (the #102 method), download the artifact, and land the two files the same way — the diff shows only the three added lines.
- [ ] **Step 4:** Run `cargo test -p ofs-sim --test radio_golden` on both OSes locally — Expected: pass. Note in the PR that CI's first `macos` run after the merge is the real arbiter for the two macOS files.
- [ ] **Step 5:** Commit `test(sim): radio goldens for the toothpick, race and long-range quads (#140)`.

### Task 6: The docs catch up — the evidence server build and the audio limits

#119's final-review triage left "the mid-write doc names tool versions but not the server build beside 'a debug server'", and #162's known limits (ground-speed airspeed, the aliasing ceiling, the single thud) are recorded only in its spec.

**Files:**
- Modify: `docs/research/mcap-determinism.md:83`
- Modify: `docs/roadmap.md` §Video (after the DVR bullet, around line 57)

**Interfaces:** none.

- [ ] **Step 1:** Verify which build ran the #48 evidence: the gate table in #119's body names the range `c2ba78c..60a8fe0` — confirm with `git show --stat 60a8fe0` that it is on the 0.3.0 line, then replace "a debug server built from this repository" with "a debug `ofs-sim` built from this repository at `60a8fe0` (the 0.3.0 branch)". If the gate record does not support naming a commit, name the branch and say so — never invent precision in a research doc.
- [ ] **Step 2:** Add the audio known-limits bullet to `docs/roadmap.md` §Video: flight audio (#158) uses ground speed as airspeed (no wind model yet), one thud for every collision kind, harmonics alias above about 7.35 kHz blade-pass, and its samples are only bit-identical on one platform (libm; #159's goldens must not expect byte equality).
- [ ] **Step 3:** Commit `docs: name the mid-write evidence build; record the audio model's limits`.
