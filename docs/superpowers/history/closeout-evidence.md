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
answers; the roadmap records it either way.
