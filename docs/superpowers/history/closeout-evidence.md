# v1 close-out evidence

Notes the close-out (spec `docs/superpowers/specs/2026-10-13-v1-closeout-design.md`) gathered while closing carried
items, kept so the roadmap's one-line verdicts can cite them.

## The m3c VTX timeout

`test_moving_off_the_parked_quads_channel_lowers_the_interference` timed out once (`no MSP reply to command 89`,
run 38078545752, 2026-10-10). Searched: every ci.yml run from 2026-10-10 to 2026-10-13 — 7 runs (38043527857 to
38440416805), all 7 with logs still available. Recurrences: none besides 38078545752 itself. Verdict: closed — not
seen again in the 6 later runs, each of which ran the test.
