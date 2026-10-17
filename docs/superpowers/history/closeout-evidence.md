# v1 close-out evidence

Notes the close-out (spec `docs/superpowers/specs/2026-10-13-v1-closeout-design.md`) gathered while closing carried
items, kept so the roadmap's one-line verdicts can cite them.

## The m3c VTX timeout

`test_moving_off_the_parked_quads_channel_lowers_the_interference` timed out once (`no MSP reply to command 89`,
run 38078545752, 2026-10-10). Searched: every ci.yml run from 2026-10-10 to 2026-10-13 — 7 runs (38043527857 to
38440416805), all 7 with logs still available. Recurrences: none besides 38078545752 itself. Verdict: closed — not
seen again in the 6 later runs, each of which ran the test.

## A client that hangs up right after MSP_REBOOT (found by Task 11)

Writing the PID-persistence test, the first version sent `MSP_REBOOT` and closed its TCP connection at once: Betaflight
never rebooted and SITL stopped answering (`no motor output within 500 ms`, a `FirmwareCrashed` session), three runs in
three. Keeping the connection open until the relaunch (as `test_the_osd_and_vtx_survive_a_betaflight_reboot` does)
passes. Betaflight reboots only once the command's reply has gone out, and with the connection gone it appears to wait
for good. Whether the Betaflight Configurator hangs up that early after a Save is what the maintainer's manual check
answers: it does not — the check (2026-10-14) connected the real desktop app, saved a PID change that
survived a reconnect, and the session stayed healthy throughout (radio up, zero overruns, restarts 0; the
PID tab's Save does not reboot Betaflight at all).

**Fixed in M7a (#57).** The cause was in SITL's TCP serial layer. Betaflight's MSP reboot calls
`waitForSerialPortToFinishTransmitting` and reboots only once the port's TX buffer is empty; the patched `tcpDataOut`
returned early when no client was connected (`conn == NULL`) without draining that buffer, so after a client had
hung up the reply stayed in the buffer for good. The patch now drops the pending bytes when there is no connection, as
a UART with nothing on the wire does. `a_client_that_hangs_up_right_after_msp_reboot_does_not_stop_the_reboot`
(`crates/ofs-sim/tests/sitl_live.rs`) failed before the change (`no motor output within 500 ms`, the SITL log ending
after `[CLS]UART1` and the EEPROM save) and passed 5 of 5 runs after it.

## Betaflight SITL can abort while it resets (found by Task 11)

Once, both reboot tests of `python/tests/test_sitl_video.py` failed in the same run: after `[system]Reset!` and
`tcpThread end!!`, glibc reported `tcache_thread_shutdown(): unaligned tcache chunk detected` and SITL died of SIGABRT,
so the simulator reported a firmware crash instead of a reboot. Heap corruption inside SITL's teardown (the TCP serial
thread is the last to end), not in the simulator. It did not come back in 38 later reboots (16 runs of both reboot
tests, three runs of the whole file). Recorded in the roadmap as an open upstream-SITL item; a reboot that crashes
this way fails the session loudly, it does not corrupt anything.

## The SITL port-bind flake

Two CI failures preceded the close-out, both `could not bind its ports`, both on the first SITL launch of a fresh
process shortly after another process's session ended: run 38437153244 (tag `v0.1.0`, the wheel step's
`test_sitl_sandbox.py`, 16 s after the same test passed in the step before) and run 38440416805 (`main` 95227fd,
`crates/ofs-sim/tests/sitl_live.rs`'s first test, right after ofs-fc's `sitl_live` binary finished). Locally the
flake never showed: six cycles of CI's exact sequence and fifteen rounds of `pkill` + immediate relaunch on WSL,
all clean.

The diagnostics commit (735dcbc, `a failed diff apply is an error; a bind failure names the port and who holds it`)
went to `main` with the merged close-out code, and the sitl job was re-run six times (run 38453432260,
2026-10-13): attempts 2 and 5 failed, 1, 3, 4 and 6 passed.

- **Attempt 5** (the pytest step, `test_vtx_interference_degrades_the_picture`) — the flake, caught:
  `bind port 5761 for UART1 failed!!` followed by `port 5761: no socket found (or not visible from this platform)`.
  The failing boot's own output shows 5762/5763/5765 bound, 5761 failing, and the boot continuing past the failure
  (1055 EEPROM writes, then ready). So *something* held or broke the 5761 bind, and by the time the error was
  assembled the port was free.
- **Attempt 2** (ofs-sim's `sitl_live`) — `crsf_sticks_reach_betaflight` timed out on an MSP reply
  (`Timeout { cmd: 105, pumps: 500 }`) with the session up and three tests green around it. Consistent with the
  bridge's MSP connection landing on a previous SITL that died mid-exchange; nothing in the log proves it.

**Reading:** the evidence fits a holder that was dying at the moment of the bind. The pre-launch cleanup
(`pkill -x betaflight_SITL`, `fc.cleanup` / `OFS_SITL_CLEANUP`) signals but does not wait, so the next launch can
race its victim's last milliseconds: the new SITL bound the ports the victim had already closed (5762, 5763, 5765)
and hit the one still open (5761). The snapshot that would have named it was taken too late — fixed by ea76d8a
(`the port holders are read when the bind line arrives`): the holders are read the moment the failing line is
written to the log sink, kept in the error and in `sitl.log`; if none was seen at the bind, the port is checked
again when the error is built and a late holder is reported as such.

**Verdict: open.** The next CI failure names the holder at the moment of the bind — or proves nobody held it,
which points inside Betaflight's SITL (dyad dual-stack, a double bind), where the fix belongs in
`third_party/betaflight/ofs-sitl.patch`. Candidate fixes for the maintainer to choose between: (a) make the Linux
cleanup wait for its victims to exit (our code, small); (b) retry the launch once on this error; (c) patch
Betaflight's SITL. None shipped: a speculative fix is worse than a named cause.

**The maintainer chose (a), 2026-10-14.** The pre-launch cleanup now waits for its victims to actually die before
the new SITL binds (`await_cleanup`, `crates/ofs-fc/src/sitl/process.rs`): a victim that ignores SIGTERM is
SIGKILLed after a 300 ms grace, the whole wait gives up after 1.3 s to the launch (whose diagnostics still name
any holder), and under WSL the wait runs inside the VM, where the Windows host cannot read /proc. Pinned by
`a_victim_that_ignores_sigterm_is_sigkilled` and `the_cleanup_does_not_hang_when_nobody_survives`
(`crates/ofs-fc/tests/sitl_errors.rs`); the WSL script was checked by hand against a TERM-ignoring process
(405 ms to escalation and gone). **Verdict: fixed, pending CI** — closed when the sitl job has run without a
recurrence; should it recur, the error names the holder at the moment of the bind.

**The flake recurred on the fix branch (run 38610663296, 2026-10-14), and that occurrence rules the race out as
the whole story.** `test_weak_cell_reads_through_betaflight_telemetry` failed on `bind port 5764 for UART4
failed!!`. The previous test's SITL had been torn down cleanly (the session's Drop kills and reaps it
synchronously, and the new launch's wait found nothing recorded either way), the boot bound 5762, 5763, 5765 and
5761 successfully, and at the moment the failing line was read the snapshot and the live re-check saw **zero
sockets on 5764 in any state** — not even a TIME_WAIT. No dying victim can explain that: the cause points inside
Betaflight's SITL serial driver, whose bind is a single `dyad_listenEx` attempt (`serial_tcp.c`; SO_REUSEADDR is
set, there is no retry, and a failed bind still boots to ready). `await_cleanup` now reports what it saw at every
launch (`seen=<n> escalated=<0|1> ms=<t>` on the server's stderr), so the next occurrence says whether a victim
existed at launch at all. The remaining hardening candidate is a bounded bind retry in
`third_party/betaflight/ofs-sitl.patch` — the maintainer to decide.

## The DVR's recording plays in VLC (checked by hand, 2026-10-14)

The maintainer flew the Godot client against Betaflight SITL in real time, pressed **D** twice around a flight,
and played the resulting `.avi` from `user://recordings/` in VLC: it plays as a proper video. The file's insides
are already pinned by the e2e (`_avi_frames`: a RIFF walk to the movi list, every `00dc` chunk a JPEG) and the
unit pin `test_a_current_field_is_encoded_as_a_jpeg_at_the_dvr_quality`; this check closes the outside view — a
real player accepts the muxer's output.
