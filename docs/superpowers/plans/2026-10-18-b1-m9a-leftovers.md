# B1: M9a Leftovers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (native run, chosen by the user). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M9a deferred minors that issue #90 does not cover: the startup hint, the stale 5761 mentions, the hard-coded MSP port in the Python SITL tests, the lock ports in Linux's ephemeral range, the unserialized first-boot config, and the three named test gaps.

**Architecture:** Six small tasks, each one commit. Tasks 1–4 are Windows-runnable unit/doc changes; task 5 is a unix-only unit test of the launch machinery with a fake SITL script; task 6 adds two live SITL tests (WSL locally, CI's `sitl` job).

**Tech Stack:** Rust (workspace, MSRV 1.85), Python `ofs` test suite, Betaflight SITL (WSL) for task 6.

**Spec:** none. The scope is PR #89's "Known limits and deferred minors" body section (`gh pr view 89`), minus the three slot-exclusivity holes filed as issue #90, and the documented Windows/WSL lock-domain limitation (stays documented).

## Global Constraints

- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo test --workspace --locked` green on Windows.
- The radio goldens must not change. `cargo fmt` is not a gate; hand-indent only the touched block.
- Every new test is run red first where it can be (the live tests are run red by construction: skip the code-path assertion once).
- Portability ruling for ports: the lock base must sit below every default ephemeral range (Linux 32768–60999, Windows/macOS 49152–65535).

## Review Focus

- A machine whose OS handed out one of the old lock ports (47000–47015) as a source port must no longer be able to break slot acquisition (task 4's whole point).
- An old pre-M9a SITL binary must produce a startup error whose hint says to rebuild, not a bare "exited with" (task 1).
- A Python SITL test run while another simulator holds slot 0 must still pass: the MSP connection follows the session's reported address (task 3).
- Two simulators first-booting the same quad file at once must not both run the diff (task 5).
- A stale `config.lock` left by a killed simulator must not wedge the quad forever (task 5's staleness rule).

---

### Task 1: `startup_hint` for a SITL that rejects `--port-offset`

**Files:**
- Modify: `crates/ofs-fc/src/sitl/mod.rs` (`startup_hint`, lines 26–39)
- Test: `crates/ofs-fc/tests/sitl_process.rs` (the existing `startup_hint` test at lines ~60–70)

**Interfaces:**
- Consumes: `net::wsl_prefix(argv)`, `net::port_offset_in(argv)` (exists, `net.rs:47`).
- Produces: same signature. When the output shows Betaflight rejecting the option, the hint says the binary predates the patch and names `scripts/build-sitl.sh`.

- [ ] **Step 1: Write the failing test** beside the existing one: an argv that carries `--port-offset` (through `wsl.exe` and native forms) with output containing `Unknown argument: --port-offset` gives a hint containing `--port-offset`, `scripts/build-sitl.sh`; the same output with an argv that has no `--port-offset` gives an empty hint (an old binary launched unscoped is not this failure).
- [ ] **Step 2: Run** `cargo test -p ofs-fc --test sitl_process` — Expected: the new assertions FAIL (hint is empty today).
- [ ] **Step 3: Implement** in `startup_hint`, before the missing-program branch: if `output.contains("Unknown argument")` and `net::port_offset_in(argv).is_some()`, return the rebuild hint (one sentence, naming the patch, `scripts/build-sitl.sh`, docs/dev-setup.md). WSL is not required for this branch: a native Linux launch of an old binary hits it too, so do not gate it on `wsl_prefix`.
- [ ] **Step 4: Run** `cargo test -p ofs-fc` — Expected: pass.
- [ ] **Step 5: Commit** `fix(fc): a SITL that rejects --port-offset gets a rebuild hint`.

---

### Task 2: Stale 5761 mentions

**Files:**
- Modify: `proto/ofs/v1/sim.proto:96` (comment only), `docs/dev-setup.md:53`, `docs/dev-setup.md:306`, `docs/dev-setup.md:308`

**Interfaces:** none (comments and docs only; regenerate nothing — confirm `git diff --stat` shows no generated file).

- [ ] **Step 1: Fix the proto comment** on `LoadReply.configurator_address` to name the slot: "the slot's Configurator address (tcp://127.0.0.1:5761 for slot 0) when the FC is Betaflight SITL, else empty".
- [ ] **Step 2: Fix `docs/dev-setup.md`**: line 53 keeps the example but defers to the HUD ("connect to the address the HUD shows — tcp://127.0.0.1:5761 on an otherwise idle machine"); line 306's lead sentence says SITL serves MSP on the slot's port (5761 for slot 0, +10 per slot, see the table below); line 308's step 2 says enter the address the script printed, keeping 5761 only as the example.
- [ ] **Step 3: Grep** `grep -rn "5761" docs/ proto/ godot/ python/ | grep -v "+ 10 n"` and fix any other mention that states 5761 unconditionally (the table at dev-setup.md:373–375 and the slot-0 examples are correct as they are).
- [ ] **Step 4: Run** `cargo test --workspace --locked` — Expected: pass (comments only).
- [ ] **Step 5: Commit** `docs: the Configurator address is the slot's, not always 5761`.

---

### Task 3: The Python SITL tests follow the session's MSP port

**Files:**
- Modify: `python/tests/test_sitl_video.py` (`MSP_PORT = 5761`, line 17; the `Msp.__init__` connect), `python/tests/test_sitl_tramp.py` (same, line 16)

**Interfaces:**
- Consumes: `sim.configurator_address` (the Python client already stores it at load, `python/ofs/client.py:430`, format `tcp://127.0.0.1:5781`).
- Produces: `Msp` connects to the port parsed from the session's address; the module-level `MSP_PORT` constant is gone.

- [ ] **Step 1: Edit both files**: delete the `MSP_PORT` constant; in `Msp.__init__`, replace the hard-coded port with `int(sim.configurator_address.rsplit(":", 1)[1])` (the session must be loaded before `Msp` is built — both tests already do; add a one-line comment saying the port is the session slot's).
- [ ] **Step 2: Grep** both files for other uses of `MSP_PORT` (assertions on the port number) and route them through the parsed value or drop them.
- [ ] **Step 3: Run in WSL** (PowerShell, the dev-setup option-2 recipe from the conventions memory):
  `wsl -d Ubuntu -- bash -lc "cd /mnt/c/dev/open-fpv-sim && OFS_SITL_LAUNCH='<the dev-setup launch line>' PYTHONPATH=python uv run --with grpcio,protobuf,pytest python -m pytest python/tests/test_sitl_video.py python/tests/test_sitl_tramp.py -q"`
  Expected: pass. The red proof: temporarily parse port 5762 in one file and see the connect time out; restore.
- [ ] **Step 4: Commit** `test(python): the MSP tests connect to the session's slot port, not 5761`.

---

### Task 4: The lock ports leave Linux's ephemeral range

**Files:**
- Modify: `crates/ofs-fc/src/sitl/slot.rs` (`LOCK_PORT_BASE`, line 23, and the module doc's `47000`), `docs/dev-setup.md` (the "loopback UDP lock on 47000 + n" sentence in the Ports and parallel sessions section), any other `47000` grep hit

**Interfaces:**
- Produces: `LOCK_PORT_BASE: u16 = 21_600` (ruling: below every default ephemeral range, unassigned by IANA to a common service; slots lock 21600–21615). All tests derive from the constant (`slot.rs` `space()` adds 100·n), so no test number changes.

- [ ] **Step 1: Change the constant and every doc mention** (slot.rs module doc, dev-setup.md). Grep `grep -rn "47000" . --exclude-dir=target --exclude-dir=.git` first and list the hits in the commit message.
- [ ] **Step 2: Run** `cargo test -p ofs-fc --lib` (slot unit tests) and `cargo test --workspace --locked` — Expected: pass unchanged (the tests are constant-relative).
- [ ] **Step 3: Commit** `fix(fc): the slot locks move to 21600+n, below every ephemeral port range`.

---

### Task 5: Serialize the first-boot diff between two simulators

**Files:**
- Modify: `crates/ofs-fc/src/sitl/process.rs` (`launch`, lines 146–175: the `apply_diff` region)
- Test: `crates/ofs-fc/tests/sitl_process.rs`

**Interfaces:**
- Produces: a `config.lock` file held in the workdir while the diff applies. A second `start()` of the same workdir waits (poll 20 ms) for the lock to disappear or `eeprom.bin` to appear, up to `cfg.startup_timeout`; a lock whose mtime is older than 300 s (`LOCK_STALE`) is removed and the wait restarts once.

- [ ] **Step 1: Write the failing test** (`#[cfg(unix)]`, the file's sh-script pattern): a fake SITL script that, when `eeprom.bin` is missing, sleeps 300 ms, prints `[CONFIG] Config file processed, EEPROM saved`, writes `eeprom.bin`, appends one line to a counter file, then prints `[SITL] ready for the simulator` and sleeps 30 s (as the running SITL); when `eeprom.bin` exists it skips straight to the ready line. Two threads call `SitlProcess::start` with the same `LaunchConfig` (same workdir, `cleanup: vec![]`, startup_timeout 30 s). Assert both are `Ok`, the counter file has exactly one line (the diff applied once), and drop both.
- [ ] **Step 2: Run** `cargo test -p ofs-fc --test sitl_process` — Expected: FAIL (counter has two lines: both threads run `apply_diff`; they may also interleave badly).
- [ ] **Step 3: Implement** the lock in `launch`: before the `eeprom.bin` check, `OpenOptions::new().write(true).create_new(true).open(workdir.join("config.lock"))` — on success the caller holds it (drop at the end of `launch`'s config region, i.e. keep it in a local that outlives `apply_diff`); on `AlreadyExists`, loop: if `eeprom.bin` exists, break (the winner finished); else if the lock's mtime is older than `LOCK_STALE`, remove it and retry `create_new` once; else sleep 20 ms and re-check, giving up at `startup_timeout` with a `FcError::Config` naming `config.lock`.
- [ ] **Step 4: Run** `cargo test -p ofs-fc` — Expected: pass, including the new test red→green. (Windows skips the unix test; the lock code itself runs everywhere.)
- [ ] **Step 5: Run** `cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: clean (the house rule this plan's executor must not skip).
- [ ] **Step 6: Commit** `fix(fc): a quad's first boot serializes its diff behind config.lock`.

---

### Task 6: The two named live test gaps

**Files:**
- Modify: `crates/ofs-sim/tests/sitl_live.rs` (add two `#[ignore]` tests; CI's `sitl` job runs `--test sitl_live -- --ignored`; the file is `#[cfg(target_os = "linux")]` overall — follow its existing helper for loading a SITL quad and reading a session's `configurator_address`)

**Interfaces:**
- Consumes: the session's `configurator_address` (proto `LoadReply`, `tcp://127.0.0.1:5771` for slot 1); a held UDP socket on `127.0.0.1:9002` (slot 0's motor port) to force slot 1; the repro-bundle replay pattern from `crates/ofs-sim/tests/repro.rs`.

- [ ] **Step 1: Write `a_session_whose_first_slot_is_held_lands_in_the_next`**: bind a throwaway UDP socket on `127.0.0.1:9002`, load the SITL quad (lockstep, short run), assert the session works and its `configurator_address` is `tcp://127.0.0.1:5771` (slot 1), then drop the socket. Red proof: run it before writing the test's assertion against the slot and see it report 5761 — in practice just assert first and see it pass; the mutation proof is to delete the held socket and see the address become 5761.
- [ ] **Step 2: Write `a_bundle_from_a_session_in_slot_n_replays`**: with slot 0 held as above, run 2 s of armed flight so the session fails or is stopped into a bundle (follow the file's existing bundle-producing test; if none stops cleanly, reuse the kill-and-bundle pattern), then `replay(&folder, scratch)` and assert `outcome.reproduced` (or the file's established assertion for a clean stop) — the point is that nothing in the bundle path assumes slot 0.
- [ ] **Step 3: Run in WSL** (PowerShell): `cargo test -p ofs-sim --test sitl_live --locked -- --ignored --test-threads=1` — Expected: the whole file passes, including the pre-existing tests.
- [ ] **Step 4: Commit** `test(sim): a held slot 0 pushes a live session to slot 1, and its bundle replays`.

---

## Not in this plan

- The three slot-exclusivity holes (TCP ports unprobed, reload race, hidden bind errors): issue #90.
- The Windows/WSL lock-domain split: documented design limit (`slot.rs` module docs, dev-setup.md).
- `--port-offset` discovery in `startup_hint` for the timeout path: only the exit path prints hints today; a hung old binary already says "is it the lockstep build from scripts/build-sitl.sh?".

## Self-Review

- **Coverage:** the PR #89 deferred list minus #90's three items: startup hint (1), 5761 mentions (2), hard-coded test ports (3), ephemeral-range locks (4), concurrent builds (5), held-motor-port product path and slot-n bundle (6). `sim.proto:96` and `dev-setup.md:53,306` are the PR's named sites; task 2's grep sweeps for the rest.
- **Placeholders:** task 6's bundle-producing step names the two patterns to copy and the assertion rule; everything else carries exact sites.
- **Interfaces:** `startup_hint` keeps its signature; `LOCK_PORT_BASE` is consumed only through the constant; `Msp` keeps its constructor shape (port becomes derived).
