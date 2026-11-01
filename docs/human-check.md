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

## The Consequence-completion plan set — before it merges

- **Rule the specs' maintainer questions** (`docs/superpowers/specs/2026-10-31-{rotated-boxes,object-collisions,crash-damage}-design.md`):
  - object-collisions — #29's "props with each other" is read as two aircraft's props, ruled out of part 1 (one
    vehicle per session); the executing session posts this reading in #156 before starting and proceeds unless
    corrected — say the word here first if you disagree.
  - crash-damage — a dislodged battery sags one cell (the `WeakCell` seam holds one; pack-wide sag would be a
    tenth fault variant), and a catastrophic impulse floors a motor at `scale = 0.2` rather than killing it (the
    spec's two open questions).
  - rotated-boxes — none (yaw-only declined full orientation with the `rooted_on` exactness reason).
- **Read the dispositions.** The umbrella (`docs/superpowers/plans/2026-10-31-consequence-completion.md`)
  propositions the wave and its deltas; the wire-number coordination rule (three pairs assume protocol 17, two
  collide on `WorldObject` field 9, two on `State` field 21 — sequential landing, next free number) is the one to
  keep in view.
- **Rule the filed follow-ups:** #174 (the hover-determinism flake on windows-sitl — one strike, passes since)
  and the minors' two rulings (the ESC rating stops clamping #164, with the golden regeneration it sanctions;
  the mag block is SITL-side only #165).

## At execution — the Consequence tasks' by-hand passes

- rotated-boxes: the yawed gate bar in meadow read at a glance in both cameras; a yawed wall's crash feel
  unchanged from an axis-aligned one of the same shape (its plan's docs task).
- object-collisions: the park crate's throw crossing the diamond visibly; a crate strike's toast; the timing
  row's numbers (its plan's Task 7).
- crash-damage: a crashed quad flying audibly and visibly worse before repair and right after (Python `repair`),
  the bent prop's shake audible in a recording, the resting-forever quad staying factory-new (its spec's
  checked-by-eye list).
- the minors: the 5F's full-throttle amperage against a bench expectation if one is ever available (#164's
  number is model-derived); a mag quad's compass in the Betaflight Configurator following declination (#165).

## Standing

- #163 stays open until both waves' checks are in; this file points, #163 keeps.
