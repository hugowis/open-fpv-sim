# macOS, issue #68

Findings of the M9c survey (2026-10-19). Plan: `docs/superpowers/plans/2026-10-19-m9c-macos-survey.md`.

There is no Mac on the maintainer's machine, so the survey ran on GitHub's macOS runners, through
`.github/workflows/macos-survey.yml` (manual: Actions, "macos-survey", Run workflow; every step is allowed to fail and
is reported in the job summary, the logs are an artifact). Two runners: `macos-latest` (Apple silicon: macOS 26.6.2,
arm64, Apple clang 21) and `macos-15-intel` (macOS 15.7.9, x86_64, Apple clang 17); both with Rust 1.99.1 and Python
3.12.10. No physical Mac was touched: nothing here says anything about a window, a GPU or a game controller.

## The list #68 asked for
First run (run 39289050136, nothing changed yet) and the last run of this branch (run 39301047490, with the fixes below):

| Area | First run | After this branch |
|---|---|---|
| `cargo build --workspace` (godot-rust included) and `cargo clippy -D warnings` | ok on both | ok on both |
| `cargo test --workspace` | 2 failures on both (below), 3 more on Intel | ok on both |
| `pip install -e "python[dev]"` | arm64 ok; **Intel fails** (no `rerun-sdk` 0.39 wheel) | ok on both |
| `pytest python/tests` | arm64 105 passed, 26 skipped (no SITL); Intel could not install | ok on both (SITL tests run in the next rows) |
| Godot client tests (`scripts/run-godot-tests.sh`) | **Linux only**: the script downloaded the Linux editor, and macOS' `sha512sum` rejects GNU flags | Both runners, run 39479683784: `unit` 362 checks, `open-loop`, `errors` and `betaflight` all pass; the open-loop DVR line is "83 frames for 83 rendered field boundaries" on Apple silicon and "81 for 81" on Intel. The survey runs the four suites as separate steps, so a failure no longer hides the later ones |
| Betaflight SITL, stock | builds (35 to 123 s across runs) | builds |
| Betaflight SITL, with `ofs-sitl.patch` | **does not build**: `pthread_condattr_setclock` does not exist on macOS | builds with `ofs-sitl-macos.patch` (two hunks, one wait) |
| Live tests against the patched SITL (`ofs-fc`, `ofs-sim`, `ofs-video`) | not reachable | pass on both (the held-port test is replaced on macOS, see below) |
| Python SITL tests (26 + the orphan-SITL test) | not reachable | all pass on both |
| Slot cleanup after a hard-killed simulator | blind (below) | works (test added) |

Real-time factors on the runners (one 20 s run, release `ofs-sim`, `scripts/benchmark.py`): Apple silicon open loop 70x
with recording and 186x without, Betaflight SITL 11.8x and 13.1x; the Intel runner 22.8x, 65x, 2.9x and 3.2x. The
Windows-native and WSL numbers of `docs/research/sitl-native-windows.md` were measured on another machine, so the
rows are not comparable.

## What the survey found, and what was done

### The SITL patch needed one macOS change
The lockstep build idles for at most 1 ms on a condition variable whose clock is `CLOCK_MONOTONIC`, set with
`pthread_condattr_setclock` so that a wall-clock step cannot lengthen the wait (WSL 2 resyncs its clock after the host
sleeps). macOS has no `pthread_condattr_setclock`. `third_party/betaflight/ofs-sitl-macos.patch` plainly initialises
the condition on macOS and waits a *relative* 1 ms with `pthread_cond_timedwait_relative_np`, which a wall-clock step cannot
stretch either; `scripts/build-sitl.sh` applies it on Darwin (`OFS_SITL_MACOS_PATCH=none` skips it, for a patch of your
own). The first version of the patch used an absolute realtime deadline and passed every test; review pointed out it would
stall serial traffic by the size of a backward clock step, which is why it was replaced. Nothing else failed to compile
or link, `sitl.ld` is not used on Mach-O, and the `%ld` print that crashes the Windows build
(`docs/research/sitl-native-windows.md`, row 5) did not crash on Apple silicon. `nproc` does not exist: the script falls back to
`sysctl -n hw.ncpu`.

### The stray-SITL cleanup was blind on macOS
M9a's slot cleanup selects processes with `pgrep -x betaflight_SITL` and reads `/proc/<pid>/cmdline`. macOS has no
`/proc`, and the kernel's process name (what `pgrep -x` and `pkill -x` compare) is the executable name cut at 16
characters, `betaflight_SITL.` for `betaflight_SITL.elf`, while `ps -o comm` prints the full path (reasoned from the BSD
tools, and consistent with the failure; `pgrep -x` itself was not run on a runner). A SITL left behind by a hard-killed simulator was
therefore never found, the next session could not bind (`UDP port 9003 is already in use`), and every later test in the
process failed (the orphan test, added in this branch, failed on both runners before the fix). Main then gained #100's probe of the
slot's TCP ports at acquire time: on Linux an orphan now makes the next session skip its slot (so the test no longer
asserts the slot, only that the next session loads and flies and is never B's), but on macOS the probe cannot see the
orphan, because it binds `127.0.0.1` with `SO_REUSEADDR` and the orphan listens on the wildcard address, which BSD allows
(the next session took slot 0 in CI run 39312234434): there the cleanup below is what removes it, and the probe still
works against a holder on `127.0.0.1` (`a_foreign_holder_of_a_tcp_port_of_the_slots_block_skips_the_slot` passes its skip
assertion on macOS). Where `/proc` is absent the script now finds the processes with `ps -ww -axo pid=,comm=` (the
basename starts with `betaflight_SITL`) and `ps -o args= -p`; the optional second argument `ps` forces that path on Linux,
where `the_slot_script_through_ps_alone_takes_the_same_victims` runs the same decoys through it (the macOS full-path-with-spaces
case itself is untested: the Linux decoys see the short name). The cleanup of a launch with no slot at all (the hand-made launches of
the live tests) was `pkill -x betaflight_SITL`, blind the same way; on macOS it is now `SITL_KILL_ALL_SCRIPT`, the same
`ps` search, tested on Linux through its `ps` argument (`the_unscoped_script_through_ps_kills_every_sitl_and_only_those`) and
not yet seen working on a runner before this branch's last push. The wait for the victims is now the same on macOS:
`await_cleanup` runs `UNIX_WAIT_SCRIPT` through `sh` and `ps`, with the Linux path's 300 ms SIGTERM grace and 1 s SIGKILL
grace. A victim that ignores SIGTERM is therefore SIGKILLed on macOS too, and the path-with-spaces test now runs there
instead of only on Linux. Survey run 39479683784 passed `the_unscoped_wait_sigkills_a_term_ignoring_victim_on_macos` on
both runners, so the path is verified on a runner.

### macOS differences in the tests, fixed
- `touch -d "10 minutes ago"` is GNU only: `sitl_process.rs` sets the modification time from Rust.
- Two tests need a second loopback address: macOS answers only on `127.0.0.1` unless one is added with `sudo ifconfig lo0
  alias 127.0.0.2`: `replies_from_another_address_are_ignored` and (from #100)
  `a_loopback_holder_does_not_push_a_session_on_another_address_off_its_slot` are ignored on macOS with that reason.
- A held TCP port did not stop SITL's own bind on macOS: `a_held_serial_port_is_retried_then_refused_with_the_errno` held
  `127.0.0.1:5761` and SITL started anyway. The likely reason is that BSD `SO_REUSEADDR` lets a wildcard bind coexist with a
  specific-address listener (untested: a holder on `0.0.0.0` also did not stop it in one run, and Rust's listener sets the
  option itself, which was not isolated). The live test is now not compiled on macOS; `a_held_tcp_port_does_not_stop_sitl_on_macos`
  pins that behaviour instead of returning early. `a_sitl_that_reports_a_spent_bind_fails_with_the_errno` covers the
  retry/errno error mapping on every OS, so the macOS gap loses no coverage. A stale SITL is still caught by the simulator's
  probe of UDP 9003 (the orphan test shows it doing so).
- The radio goldens are now named per architecture on macOS (`elrs.macos-aarch64.digests`,
  `elrs.macos-x86_64.digests`, and the same for `radio-flight.*`); a missing file fails loudly instead of skipping. The
  four files were recorded by survey run 39474665915 (`macos-latest` and `macos-15-intel`) and the comparison passed on
  both in run 39479683784. The two `elrs` files are identical; `radio-flight`'s `freestyle` digest differs between the
  architectures, which is why one file per architecture was needed.
- Slot unit tests failed on macOS in survey runs 39289050136 (three tests, Intel), 39293693518 (two, Intel) and
  39295539454 (one test, **both** runners): after a slot was dropped the same slot was sometimes still taken a moment later.
  The arm64 job passed them in the other runs. The tests now retry for up to 2 s, on macOS only (`acquire_until`), and passed
 in the one survey run after that. The cause is not established; the likeliest is a child process that another test
 thread was starting at that instant inheriting the descriptor before it was marked close-on-exec. The 50-run harness
 (`scripts/macos-slot-stress.sh`, the survey's `stress_slot` input) is the pending check: it either reproduces the flake
 (the failure path prints `lsof` for the motor port) or records that the retry is the accepted mitigation. The retry stays
 until that run happens.
- `late_duplicate_replies_are_drained_after_a_resend` (a 30 ms thread against a 150 ms window) failed on arm64 in run
  39297354882 (it passed in the runs before and after): the window is 500 ms. It has had one survey run since.
- `test_the_recording_reports_its_events_and_refuses_misuse` read the event stream once, before its background thread had
  delivered the second event (failed on both runners in one run, passed in another): it waits for both events now.
- Intel Macs: `rerun-sdk` 0.39 publishes no x86_64 macOS wheel, so the `[dev]` extra skips it there
  (`test_rerun.py` skips itself without it).

### Godot
`scripts/run-godot-tests.sh` knows the macOS editor (`Godot_v4.7.2-stable_macos.universal.zip`, its SHA-512 pinned from the
release's `SHA512-SUMS.txt`, which also contains the Linux hash already pinned), uses `shasum` where macOS' `sha512sum`
refuses GNU options, supplies a `timeout` (`gtimeout`, or a perl alarm) because macOS has none, and `godot/ofs.gdextension`
lists `libofs_godot.dylib` for `macos.debug` and `macos.release`. (Whether the runners used `gtimeout` or the perl stand-in is
not recorded; the stand-in signals only the editor, not its children.) On Apple silicon the unit suites (362 checks),
the open-loop and error end-to-end tests and the end-to-end test with Betaflight all passed in the one survey run that ran
them (39301047490). On the Intel runner, in that run, the unit suites passed and the open-loop end-to-end test failed one check:
"44 frames for 2.01 simulated seconds at 50 fields a second (about 100)". That check multiplied simulated time by the
nominal 50 Hz, so it measured the runner's speed: the DVR writes one frame per field the client actually renders. It now
counts the distinct `target_field` values seen between DVR start and stop and allows one boundary of skew; on Linux it
reports "101 frames for 101 rendered field boundaries". The four suites now run as separate steps. In run 39479683784 all
four passed on both runners (Intel open-loop: "81 frames for 81 rendered field boundaries"); in the generation run
39474665915, which used the same DVR check, `betaflight` still reported 2 real-time overruns on Apple silicon and 32 on
Intel while its other checks passed, so that suite's timing remains the flaky one.

## Decision
**Apple silicon is supported best-effort, and CI now has a `macos` job for it** (`ci.yml`, `macos-latest`: the Rust tests, the
live Betaflight tests, every Python test with SITL, the Godot suites with the game flying Betaflight). It is not in
`publish`'s `needs`, and it is `continue-on-error`: it shows green even when it fails, until it has a track record (about ten
green runs), because several of the tests it runs had only one to four survey runs and two of them flaked once. Its first
measured run and duration: see the line at the end of this section. **Intel Macs** passed every Godot suite in run
39479683784 once the DVR check measured rendered fields; the Intel runner is about 3 to 4 times slower than Apple silicon
in the benchmark. `macos-15-intel` is run only by the manual survey workflow.

First measured run of the `macos` job: run 39307557811 (commit 9d93847), every step passed, 15 min 47 s (an earlier run of the
job was cancelled by a newer push after 13 min). Its record after that is mixed, which is why it is non-blocking: on the
final commit the `pull_request` run (39315660145) passed and the `push` run of the same commit (39315650666) failed, in
the Godot open-loop end-to-end test on Apple silicon, on two real-time checks ("a clean link holds nothing: held_fields is 0" and
"79 frames for 2.01 simulated seconds at 50 fields a second (about 100)", the DVR needing at least 80 %). Those Godot
checks are paced by the wall clock and have now failed once on each runner type; the earlier failures of the job were
tests that `main` gained (#100) or that were fixed in this branch (listed above).

## A physical Mac
Not run yet: no physical Mac has been tried. The checklist for the window, Metal renderer, gamepad probe, Gatekeeper
quarantine and the source build is `docs/research/macos-manual.md`; its result table is the record to copy here.

Not done, and why:
1. **A physical Mac.** The window, Metal renderer, gamepad, and Gatekeeper quarantine are untested. Follow
   `docs/research/macos-manual.md` on a real machine and copy its table into this file; the CI runners cannot answer
   these questions.

**Distribution: source build, no notarised binary (decided).** The `ofs` Python package is pure Python and installs
with `pip install -e "python[dev]"`; `ofs-sim` is built from source with `cargo build -p ofs-sim --locked`. A
signed and notarised `.app` or universal binary is not planned: it needs an Apple Developer ID, a notarisation
step in the release workflow, and a decision about the Godot editor download that the current release does not
have. Reopen this only with an Apple Developer account and a release-workflow owner.

The `macos` job is still not promoted: of the last ten `ci.yml` runs on `main`, only run 39459360292 has the job (the
earlier nine predate it), and it failed one step, `Run bash scripts/run-godot-tests.sh all`. That step is now split into
named suites and its DVR check counts rendered field boundaries, so the next ten runs are the new record. The branch's
survey run 39479683784 is clean on both runners (all four Godot suites included), and its `ci.yml` run 39479674930 is
green with no failed macOS step, but one branch run does not replace the `ci.yml` record. It remains `continue-on-error` and outside `publish.needs` until that record is clean; `ci.yml` is
deliberately unchanged in this branch.
