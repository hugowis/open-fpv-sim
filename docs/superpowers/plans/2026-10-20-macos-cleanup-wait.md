# macOS cleanup wait and slot-test stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `await_cleanup` wait for, and if necessary SIGKILL, a stale SITL on macOS, prove the `ps` path (including paths with spaces), and settle the slot-test flake that is currently masked by a two-second retry.

**Architecture:** Add a portable shell wait script compiled from the existing `slot_pids_sh!` helpers. macOS runs that script with `sh -c` and the `ps` argument; Linux keeps the native `/proc` scan. The script reports the same `seen=<n> escalated=<0|1> ms=<t>` line the WSL wait already uses, so `parse_wait_output` and `AwaitCleanup` do not change. Enable the cleanup tests on macOS, add a forced-`ps` Linux test, and add a stress harness that either reproduces the slot flake or records that the retry is the accepted mitigation.

**Tech Stack:** Rust 1.85 (workspace MSRV), POSIX `sh`, BSD `ps`, Cargo, GitHub Actions macOS runners.

**Spec:** PR #101's "Not done / deferred minors" (`docs/research/macos.md`, sections "The stray-SITL cleanup was blind on macOS" and "Not done, and why"), issue #68.

## Execution base

This plan is written against `origin/main` at `341ad51` (the merge of PR #101). The current `worktree-slot-exclusivity` checkout predates M9c and does not contain `docs/research/macos.md` or `scripts/macos-survey.sh`; start from a fresh worktree:

```bash
git fetch origin
git worktree add ../ofs-macos-cleanup -b worktree-macos-cleanup origin/main
```

## Global Constraints

- Linux and native Windows behaviour must stay byte-for-byte unchanged; every macOS-only path is behind `cfg(target_os = "macos")` or an explicit `ps` argument.
- No new crates, no `unsafe`.
- MSRV is 1.85. `cargo check --workspace --all-targets --locked --exclude ofs-godot` must pass on 1.85.
- The wait is bounded: 300 ms of SIGTERM grace, then SIGKILL, then at most 1 s of SIGKILL grace. It must never make a launch hang.
- Tests must not silently pass on macOS. A platform limitation is either a real macOS test that pins the observed behaviour or a test that is not compiled there, with the reason in `docs/research/macos.md`.
- The macOS CI job is `continue-on-error`; inspect its step conclusions, not the job conclusion, when validating this plan (see the CI plan `2026-10-20-macos-ci-intel-and-gating.md`).
- Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` before the final commit.

## Review Focus

- **macOS process naming.** The kernel name is `betaflight_SITL.` (16 characters), while `ps -o comm=` prints the full path. The `ps` search must match on the basename prefix, not `pgrep -x`.
- **Paths with spaces.** A SITL under `/Users/me/My Projects/...` must still be found and killed. The `ps` path must not split the command line on spaces.
- **Slot scope.** A scoped cleanup must kill only `--port-offset <n>` (and, for slot 0, a process with no option); the wait must not kill the other simulator.
- **TERM-ignoring victim.** A victim that traps SIGTERM must be SIGKILLed and the wait must return `escalated=true`.
- **Nobody to wait for.** The no-victim path must return `AwaitCleanup::NONE` quickly; it is the common case on every launch.
- **Inherited descriptors.** The slot flake hypothesis is that another test thread's child briefly held the released socket. If that is true, the stress harness must show it; if not, the retry stays and the docs say the hypothesis was not reproduced.

---

### Task 1: A portable macOS wait script

**Files:**
- Modify: `crates/ofs-fc/src/sitl/process.rs` (add `UNIX_WAIT_SCRIPT` after `WSL_WAIT_SCRIPT`; change `await_cleanup`)
- Modify: `crates/ofs-fc/tests/sitl_errors.rs` (new forced-`ps` test; enable the macOS-capable cleanup tests)
- Modify: `docs/research/macos.md` (the cleanup-wait findings)
- Test: `crates/ofs-fc/tests/sitl_errors.rs`

**Interfaces:**
- Consumes: `net::slot_pids_sh!()` from `crates/ofs-fc/src/sitl/net.rs`, which defines `slot_candidates`, `slot_args`, `slot_state`, and `slot_pids`. `slot_pids <offset> <ps>` returns the live pids of one slot; `slot_candidates <ps>` returns every candidate SITL pid.
- Produces: `pub const UNIX_WAIT_SCRIPT: &str`. Invocation: `sh -c UNIX_WAIT_SCRIPT sh <s|x> <offset> ps`. It prints exactly one last line `seen=<n> escalated=<0|1> ms=<t>`.
- Produces: `await_cleanup` uses `UNIX_WAIT_SCRIPT` on macOS and `await_cleanup_native` on Linux; other targets keep `AwaitCleanup::NONE`.

- [ ] **Step 1: Write the failing forced-`ps` test**

Append this test next to `the_wsl_wait_script_is_scoped_to_its_slot_too` in `crates/ofs-fc/tests/sitl_errors.rs`:

```rust
/// The wait script macOS runs (there is no `/proc`): the same decoys, forced through the `ps` path on Linux.
/// It sees a TERM-ignoring slot-1 SITL, escalates to SIGKILL, leaves slot 0 alone, and reports what it did.
#[cfg(target_os = "linux")]
#[test]
fn the_macos_wait_script_escalates_and_is_scoped() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let trap = |ready: &std::path::Path| format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display());
    let (ready0, ready1) = (dir.path().join("ready0"), dir.path().join("ready1"));
    let mut slot0 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready0), 0);
    let mut slot1 = spawn_fake_sitl_in_slot(dir.path(), &trap(&ready1), 10);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !(ready0.exists() && ready1.exists()) {
        assert!(std::time::Instant::now() < deadline, "the fake SITLs never installed their traps");
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::UNIX_WAIT_SCRIPT, "sh", "s", "10", "ps"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("seen=1 escalated=1"), "slot 1's victim was seen and SIGKILLed: {text:?}");
    assert!(!is_alive(&mut slot1), "slot 1's SITL is gone");
    assert!(is_alive(&mut slot0), "slot 0's SITL is left alone");
    let none = std::process::Command::new("sh")
        .args(["-c", ofs_fc::sitl::process::UNIX_WAIT_SCRIPT, "sh", "s", "20", "ps"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&none.stdout).contains("seen=0"), "no slot-2 victims: {:?}", none.stdout);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-fc --test sitl_errors the_macos_wait_script_escalates_and_is_scoped -- --exact`
Expected: compile error `no UNIX_WAIT_SCRIPT in ofs_fc::sitl::process`.

- [ ] **Step 3: Add `UNIX_WAIT_SCRIPT` and route macOS through it**

In `crates/ofs-fc/src/sitl/process.rs`, immediately after `WSL_WAIT_SCRIPT`, add:

```rust
/// The wait for a launch on a Unix host with `ps` instead of `/proc` (macOS). `slot_pids_sh!` supplies the same
/// candidate/argument/state helpers the cleanup uses; `$1` is `s` for one slot and `x` for every SITL, `$2` is the
/// slot's `--port-offset` (`0` when unused), and `$3` is `ps`. The loop counts its 10 ms sleeps instead of using
/// `date +%s%N` (BSD `date` has no `%N`), so the report's `ms` is accurate to one poll.
pub const UNIX_WAIT_SCRIPT: &str = concat!(
    net::slot_pids_sh!(),
    "\
slot_all() {
  for p in $(slot_candidates \"$1\"); do
    [ \"$(slot_state \"$p\" \"$1\")\" = Z ] && continue
    echo \"$p\"
  done
}
pids() { if [ \"$1\" = s ]; then slot_pids \"$2\" \"$3\"; else slot_all \"$3\"; fi; }
live() { [ -n \"$(pids \"$@\")\" ]; }
slay() { p=$(pids \"$@\"); [ -z \"$p\" ] || kill -9 $p 2>/dev/null; }
live \"$@\" || { echo 'seen=0 escalated=0 ms=0'; exit 0; }
ms=0
i=0; while live \"$@\" && [ $i -lt 30 ]; do sleep 0.01; i=$((i+1)); done
ms=$((ms + i * 10))
esc=0
if live \"$@\"; then
  slay \"$@\"
  esc=1
  i=0; while live \"$@\" && [ $i -lt 100 ]; do sleep 0.01; i=$((i+1)); done
  ms=$((ms + i * 10))
fi
echo \"seen=1 escalated=$esc ms=$ms\"
exit 0"
);
```

Then replace the runtime tail of `await_cleanup` with:

```rust
    #[cfg(target_os = "macos")]
    {
        let mut argv: Vec<String> = ["sh", "-c", UNIX_WAIT_SCRIPT, "sh"].into_iter().map(String::from).collect();
        match scope {
            Some(offset) => {
                argv.push("s".into());
                argv.push(offset.to_string());
            }
            None => {
                argv.push("x".into());
                argv.push("0".into());
            }
        }
        argv.push("ps".into());
        let output = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).output();
        return parse_wait_output(output.ok().map(|o| o.stdout).unwrap_or_default());
    }
    #[cfg(target_os = "linux")]
    return await_cleanup_native(scope);
    #[cfg(all(not(target_os = "linux"), not(target_os = "macos")))]
    {
        let _ = scope;
        AwaitCleanup::NONE
    }
```

Update the `await_cleanup` doc comment: replace "on plain Windows ... so this reports nothing there" with a sentence saying macOS runs `UNIX_WAIT_SCRIPT` through `sh` and `ps`, and Windows still reports nothing.

- [ ] **Step 4: Run the forced-`ps` test**

Run: `cargo test -p ofs-fc --test sitl_errors the_macos_wait_script_escalates_and_is_scoped -- --exact`
Expected: PASS. It prints no `seen=1 escalated=1` failure and both slot assertions hold.

- [ ] **Step 5: Run the Linux cleanup suite**

Run: `cargo test -p ofs-fc --lib sitl:: -p ofs-fc --test sitl_errors -- --test-threads=1`
Expected: PASS. The existing `the_wsl_wait_script_is_scoped_to_its_slot_too`, `the_sigkill_escalation_only_reaches_its_own_slot`, and `a_victim_that_ignores_sigterm_is_sigkilled` still pass.

- [ ] **Step 6: Make the path-with-spaces and scoped-escalation tests run on macOS**

In `crates/ofs-fc/tests/sitl_errors.rs`:

1. Change `#[cfg(target_os = "linux")]` on `impl FakeSitl` and `impl Drop for FakeSitl` to `#[cfg(unix)]`, so the drop guard reaps the child on macOS too.
2. Change `#[cfg(target_os = "linux")]` on `spawn_fake_sitl`, `spawn_fake_sitl_in_slot`, and `spawn_fake_sitl_with` to `#[cfg(unix)]` (the macOS-only test and the no-survivor test call `spawn_fake_sitl` directly).
3. Change `#[cfg(target_os = "linux")]` on `fn is_alive` to `#[cfg(unix)]`.
4. Change `#[cfg(target_os = "linux")]` on `a_slots_sitl_is_found_when_its_path_has_spaces` to `#[cfg(unix)]`.
5. Change `#[cfg(target_os = "linux")]` on `the_sigkill_escalation_only_reaches_its_own_slot` to `#[cfg(unix)]`.
6. Change `#[cfg(target_os = "linux")]` on `the_cleanup_does_not_hang_when_nobody_survives` to `#[cfg(unix)]`.

The path-with-spaces test now exercises `default_cleanup` plus `await_cleanup` on macOS, where both use the `ps` path; on Linux it keeps the `/proc` path. The scoped-escalation test exercises the macOS wait script's SIGKILL branch.

- [ ] **Step 7: Add a macOS-only unscoped escalation test**

Append this test after `a_victim_that_ignores_sigterm_is_sigkilled` in `crates/ofs-fc/tests/sitl_errors.rs`:

```rust
/// The unscoped macOS path has no `--port-offset`: the wait must see every SITL through `ps`, SIGKILL a
/// TERM-ignoring one, and report it. (`a_victim_that_ignores_sigterm_is_sigkilled` keeps the `/proc` assertion.)
#[cfg(target_os = "macos")]
#[test]
fn the_unscoped_wait_sigkills_a_term_ignoring_victim_on_macos() {
    let _guard = PORTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::Builder::new().prefix("my sitl ").tempdir().unwrap();
    let ready = dir.path().join("ready");
    let mut victim = spawn_fake_sitl(
        dir.path(),
        &format!("trap \"\" TERM; : > '{}'; sleep 30; sleep 30", ready.display()),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline, "the victim never installed its trap");
        std::thread::sleep(Duration::from_millis(5));
    }
    let outcome = ofs_fc::sitl::process::await_cleanup(&[dir.path().join("betaflight_SITL").display().to_string()]);
    assert!(outcome.escalated && outcome.victims_seen == 1, "{outcome:?}");
    assert!(victim.0.try_wait().unwrap().is_some(), "the SIGKILLed victim is gone");
}
```

- [ ] **Step 8: Run the full local gate**

Run: `cargo test --workspace --locked -- --test-threads=1`
Expected: PASS on Windows/Linux. The new macOS-only test is not compiled off macOS.

- [ ] **Step 9: Update the findings doc**

In `docs/research/macos.md`, replace the paragraph beginning "The native wait for the victims (`await_cleanup_native`) is
still Linux only" with a paragraph saying macOS now runs `UNIX_WAIT_SCRIPT` through `sh` and `ps` (same 300 ms SIGTERM /
1 s SIGKILL bounds as Linux), that the path-with-spaces test now runs on macOS, and that the macOS job's step
conclusions are the acceptance. If a macOS test fails, leave that outcome in the sentence rather than calling it fixed.

- [ ] **Step 10: Commit**

```bash
git add crates/ofs-fc/src/sitl/process.rs crates/ofs-fc/tests/sitl_errors.rs docs/research/macos.md
git commit -m "fix(fc): wait for and SIGKILL stale SITL on macOS (#101)"
```

---

### Task 2: Close the slot-test flake uncertainty

**Files:**
- Create: `scripts/macos-slot-stress.sh`
- Modify: `.github/workflows/macos-survey.yml` (add a `stress_slot` input and one step)
- Modify: `crates/ofs-fc/src/sitl/slot.rs` (diagnostic only, on the retry's failure path)
- Modify: `docs/research/macos.md` (record the result and the accepted mitigation)

**Interfaces:**
- Consumes: `SitlSlot::acquire_in` and the macOS-only `acquire_until` retry already in `slot.rs`.
- Produces: `scripts/macos-slot-stress.sh <runs>`; exits 0 only if every run passes. A failing run prints `lsof -nP -iUDP:<port>` for the expected motor port before the test binary exits.
- Produces: a recorded decision in `docs/research/macos.md`; no production behaviour changes unless the flake reproduces with a holder outside the test process.

- [ ] **Step 1: Add the stress harness**

Create `scripts/macos-slot-stress.sh`:

```bash
#!/usr/bin/env bash
## Re-runs the ofs-fc slot unit tests in parallel to expose the macOS port-release flake.
## Usage: scripts/macos-slot-stress.sh [runs]   (default: 20)
set -euo pipefail
runs="${1:-20}"
i=1
while [ "$i" -le "$runs" ]; do
    echo "slot stress run $i/$runs"
    cargo test -p ofs-fc --lib sitl::slot::tests --locked -- --test-threads=8
    i=$((i + 1))
done
```

Make it executable: `chmod +x scripts/macos-slot-stress.sh`.

- [ ] **Step 2: Add the survey trigger**

In `.github/workflows/macos-survey.yml`, add to the `workflow_dispatch` block:

```yaml
  workflow_dispatch:
    inputs:
      stress_slot:
        description: "Run the slot-test stress harness on Apple silicon"
        type: boolean
        default: false
```

(If the radio-goldens plan has already added `write_goldens`, keep that input and add `stress_slot` beside it.)

Then add this step immediately before the `actions/upload-artifact` step:

```yaml
      - if: inputs.stress_slot && matrix.os == 'macos-latest'
        run: bash scripts/macos-slot-stress.sh 50
```

- [ ] **Step 3: Add the failure diagnostic**

In `crates/ofs-fc/src/sitl/slot.rs`, inside `acquire_until`, replace the current `if slot.index == index || std::time::Instant::now() >= deadline` body with:

```rust
            let expired = std::time::Instant::now() >= deadline;
            if slot.index == index || expired {
                if expired && cfg!(target_os = "macos") {
                    let _ = std::process::Command::new("lsof")
                        .args(["-nP", &format!("-iUDP:{}", PORT_PWM + shift)])
                        .status();
                }
                return slot;
            }
```

This only prints on the path that already fails; it does not change the retry.

- [ ] **Step 4: Run the stress harness on macOS**

```bash
gh workflow run macos-survey.yml -f stress_slot=true
gh run list --workflow=macos-survey.yml --limit 3
```

Wait for the run, then download the artifact:

```bash
gh run download <run-id> -n survey-macos-latest -D /tmp/ofs-macos-slot-stress
rg -n "slot stress run|FAILED|panicked|AddrInUse|UDP:" /tmp/ofs-macos-slot-stress
```

Expected outcomes, both acceptable:
- **No failures in 50 runs:** the retry is the accepted mitigation; the hypothesis (a child inheriting the descriptor) was not reproduced. Record that in `docs/research/macos.md` and keep `acquire_until`.
- **A failure with an `lsof` line naming a pid outside the test process:** record the pid, its parent (`ps -o pid,ppid,command -p <pid>`), and the `lsof` output; open an issue with that evidence. Do not guess a code fix.

- [ ] **Step 5: Record the result**

In `docs/research/macos.md`, replace the sentence beginning "The cause is not established; the likeliest is..." with a paragraph that says what the 50-run stress run showed, with its run id, and either "the retry is the accepted mitigation" or "the holder was process <pid>, see issue #<n>". Do not remove the retry.

- [ ] **Step 6: Run the gate and commit**

```bash
cargo test -p ofs-fc --lib sitl::slot::tests --locked -- --test-threads=8
cargo clippy --workspace --all-targets --locked -- -D warnings
git add scripts/macos-slot-stress.sh .github/workflows/macos-survey.yml crates/ofs-fc/src/sitl/slot.rs docs/research/macos.md
git commit -m "test(fc): stress the macOS slot flake and record the verdict (#101)"
```

---

### Task 3: Verify on the macOS job

**Files:**
- No source changes; this is the acceptance gate.

- [ ] **Step 1: Push the branch and run CI**

```bash
git push -u origin worktree-macos-cleanup
gh run list --branch worktree-macos-cleanup --limit 3
```

- [ ] **Step 2: Inspect the macOS job's steps, not its job conclusion**

The job has `continue-on-error: true`, so the job can be green while a step failed. Replace `<run-id>`:

```bash
gh run view <run-id> --json jobs --jq '.jobs[] | select(.name=="macos") | .steps[] | select(.conclusion=="failure") | [.name,.conclusion] | @tsv'
```

Expected: no output. In particular, `cargo test --workspace --locked -- --test-threads=1` and the two live-test steps must have passed.

- [ ] **Step 3: Run the forced-`ps` test on Linux once more**

```bash
cargo test -p ofs-fc --test sitl_errors the_macos_wait_script_escalates_and_is_scoped -- --exact
```

Expected: PASS. This is the Linux stand-in for the macOS script; the macOS run above is the real acceptance.

## Self-review notes

- Coverage: both deferred cleanup items are covered (native wait/escalation, paths with spaces), plus the slot-flake uncertainty.
- Placeholder scan: the only decision point is the stress result; both branches have an explicit deliverable and no guessed fix.
- Type consistency: `UNIX_WAIT_SCRIPT`, `parse_wait_output`, and `AwaitCleanup` keep the existing names and types.
