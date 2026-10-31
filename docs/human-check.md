# Human checks

What only a person can do in this repository's loop: rule the judgement calls, and put eyes and hands on what
headless suites cannot render. This file is the working list; **#163** is the collector where each item lands
(permanently, as a comment) when the PR that owes it closes. An item leaves this file when it moves to #163.

## The Game-completion plan set (PR #171) — before it merges

- **Rule the specs' maintainer questions.** Each pair states them ruled-and-overridable at the end of its spec
  (`docs/superpowers/specs/2026-10-31-*.md`):
  - quad-model — the asset is authored in this repo (generator + provenance note); a sourced, licensed model is
    the alternative (spec call 2).
  - workbench — sensor parts (`imu`/`baro`/`mag`) are carried from the starting quad, not picked; a stale
    `force_battery_cell_count` diff is surfaced verbatim, not patched (the spec's open questions).
  - tour — the **T** key for in-flight replay; no re-offer after updates (the spec's open questions).
  - school — the demo replay is client-side; the school quad appears in the menu's catalog; the figure of eight
    stays in v1 (the spec's open questions).
  - world-rendering — the palette hues and the chase-view fog strength are eyeball items after execution, not
    plan questions.
- **Read the dispositions.** The umbrella propositions every open issue
  (`docs/superpowers/plans/2026-10-31-game-completion.md`); nothing executes until they look right.
- **Rule the two filed follow-ups:** #172 (the menu refusing an `open_loop_stabilized` quad under an open-loop
  setting) and #173 (a `[look]` world section) — accept, reshape or close.

## At execution — every task PR owes its by-hand pass before it merges

Each plan carries its list; fly it, eyeball it, feel the frame rate, then post it to #163 when the PR closes:

- quad-model: the chase-camera look, the FPV nose-sliver check before and after **R**, the frame rate, the menu
  preview (its plan's Task 5).
- world-rendering: the five moods in both cameras, the world reading under link breakup, the metre grid at 30+ m,
  the quality tiers differing and low holding the frame rate (its plan's Task 4).
- best-lap-ghost: the ghost through the analog chain, the new-best toast firing once, **N** mid-lap parking the
  ghost at the start (its plan's Manual QA).
- workbench: the screen's look and gamepad navigation, the `~` estimate at a glance, an assembly flying with
  Betaflight (its plan's Task 7).
- tour: a clean `user://` first run, the ten steps end to end on a real controller, both replays (its plan's
  Task 6 and the spec's checked-by-eye list).
- school: the progress numbers reading as teaching, the pylons against the circuit field, a demo flown end to
  end, a Betaflight arm on a real switch, the hover's difficulty (its plan's Task 10 and the spec's list).

## Standing

- #163 stays open until both waves' checks are in; this file points, #163 keeps.
- The Consequence wave's by-hand checks (terrain flight feel, damage by ear, DVR audio in VLC) join this file
  when that wave's plans exist.
