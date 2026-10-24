# Race timing: gates and laps

A world file can declare a course: an ordered list of gates. The simulator watches the quad's path, counts the gates it
flies through in order, and times the splits and the laps. The results are events (the Godot toasts show them), a
`race` block in every state message and `race.*` signals in a recording, so a race, a script and a test can all use
them.

## The world file

```toml
[[gates]]
name = "Gate0"               # letters, digits, '_' and '-'; unique among the gates
center_ned_m = [30.0, 0.0, -1.22]   # the middle of the opening, NED metres from home (up is negative)
heading_deg = 0.0            # the direction of travel through the gate: 0 = north, 90 = east, clockwise
width_m = 3.08               # across the direction of travel
height_m = 2.44              # the opening's height; its lowest edge must not be below the ground
pitch_deg = 0.0              # optional tilt, after the heading (see "Tilted gates"): default 0, an upright gate
roll_deg = 0.0

[[gates]]
name = "Gate1"
# ...
```

The gates are numbered in file order from 0. **Gate 0 is the start and finish.** A world has at most 64 gates, and a
world without any has no race at all (no model, no `race.*` signals, `State.race.present = false`).

An optional `[race]` table sets the rules of the race:

```toml
[race]
laps = 3                     # the race ends after this many laps; leave it out to race forever
```

`laps` must be at least 1 and needs at least one gate.

A gate is a *trigger*: it does not collide with the quad and it blocks no signal. Draw its frame with ordinary
`[[objects]]` if you want something to hit: `worlds/flat.toml` has the posts and bar of its three gates as boxes, and
the `[[gates]]` are what those leave free (3.08 m between the posts, 2.44 m under the bar). Godot draws each opening
as a translucent rectangle labelled START or with the gate's number.

## The rules

- **A pass.** Every simulation tick the path from the previous pose to the new one is tested against the *next gate
  only*. It counts when the path goes through the gate's plane in the direction of travel and the point where it does
  lies inside the opening **with room for the quad**: the quad's centre must be at least the quad's *bounding radius*
  (`QuadConfig::bounding_radius_m`: the farthest edge of its collision spheres from its origin, 0.2 m for the shipped
  5 inch) inside every edge. The radius is a sphere, so the quad's attitude does not matter. A path that grazes
  outside the opening, goes through backwards, or goes through a gate that is not next, counts for nothing and changes
  nothing.
- **A hit.** A path whose centre is inside the opening but closer to an edge than the quad's radius is a *clipped*
  pass: it raises `gate_hit` (the event's time is the crossing's), gives no credit and starts nothing; the same gate
  is still next. Like a counted pass it waits until the quad has been more than 3 m (`REARM_M`) from the gate's plane
  before the gate can be tried again, so hovering at a post raises one hit, not a stream. A quad wider than the
  opening never gets a clean pass. A centre outside the opening is a plain miss: no event (the posts, if the world
  has them as `[[objects]]`, are what the quad collides with).
- **Time.** The crossing time is interpolated inside the tick (a quad at 100 m/s crosses a plane between two 8 kHz
  ticks 12 mm apart, and the time is exact to the path's linearity), so it does not depend on the tick rate.
- **The start.** The first pass of gate 0 starts the clock: a `gate_passed` event, split 0.
- **Splits.** Gates 1 to n-1, in order, each raise `gate_passed` with the time since the lap started.
- **A lap.** Passing gate 0 again after the last gate completes a lap (`lap_completed`) and starts the next one. That
  pass raises no `gate_passed`.
- **One gate.** With a single gate every counted pass after the start completes a lap, but only once the quad has been
  farther than 3 m (`REARM_M`) from the gate's plane (along the direction of travel) since the last one, so hovering
  through the plane does not run up laps. The rule applies only to a one-gate course: a two-gate course whose gates
  face each other through one frame can still count laps while hovering there.
- **The lap limit.** With `[race] laps = N`, completing lap N ends the race: that pass raises `lap_completed` and then
  `race_finished` (same time), the clock stops (`running` false, `finished` true, `next_gate` 0), the last and best lap
  times stay, and no gate counts until the race is reset. Without a limit the race goes on.
- **A reset.** `ResetRace` (Python `sim.reset_race()`, the **N** key in the game) starts a fresh race at the next tick,
  without a new `Load`: no clock, no laps, no last or best time, not finished, gate 0 next, and the same course can be
  flown again at once. It raises `race_reset`, and the `race.*` signals go back to zero. A world without gates refuses
  it (`InvalidState`). The state is read at ticks, so a `get_state` before the next tick still shows the old race; the
  reset is part of the repro bundle's journal (`race_reset`) and replays at the same tick.
- **Not included.** No ghost or best-lap replay.

## Tilted gates

`pitch_deg` and `roll_deg` tilt the gate after its heading, the aerospace Z-Y-X order. A gate stands upright facing
`heading_deg`; a positive pitch then lifts the direction of travel (the plane leans back, the top toward the approaching
quad, like a gate for diving through from above), and a positive roll turns the opening clockwise as seen from the
approach. `pitch_deg` must be within (-90, 90) and `roll_deg` within [-180, 180]. The crossing test uses the tilted
plane and measures the opening along the tilted width and height axes, so a level path that clears an upright gate can
miss a pitched one; the "must not reach below the ground" rule applies to the tilted opening's lowest corner. Godot draws
the opening on the same plane (`World.gate_basis`), and `World.gates` carries both angles (protocol 14; Python
`ofs.Gate.pitch_deg`, `roll_deg`).

## Events

| kind | message | the event's time |
|---|---|---|
| `gate_passed` | `Gate0 (gate 0) start` for the start, else `Gate1 (gate 1) split 8.412 s` | the crossing |
| `lap_completed` | `lap 1: 31.204 s (best)`; `(best)` when it is the fastest lap so far (the first lap is) | the crossing of gate 0 |
| `gate_hit` | `Gate1 (gate 1) hit` | the crossing (a clipped pass: see "A hit") |
| `race_finished` | `race finished: 3 laps` (`1 lap`), right after the last `lap_completed` | the same crossing |
| `race_reset` | `race reset` | the tick the reset was applied at |

Seconds have three decimals. The format is stable: scripts may parse it.

## State

`State.race` (Python) / `RaceState` (protocol 11):

| field | meaning |
|---|---|
| `present` | the world has gates |
| `running` | gate 0 has started the clock and the lap limit is not reached |
| `finished` | the lap limit is reached: the clock has stopped, nothing counts until a reset |
| `lap_limit` | the world's `[race] laps` (0 = none) |
| `next_gate` | the gate to fly through next (0 before the start, and again once every gate is passed) |
| `gate_count` | gates in the course |
| `laps` | laps completed |
| `current_lap_s` | time since the current lap started (0 until the clock runs) |
| `last_lap_s`, `best_lap_s` | 0 until a lap completes (`laps > 0` says they are real) |

The recording has the same values as scalar signals `race.running`, `race.finished`, `race.next_gate`, `race.laps`,
`race.lap_start_s`, `race.last_lap_s` and `race.best_lap_s`.

In the game the HUD's lap line reads `LAP 2/3   NEXT 1   ...` with a limit and `RACE FINISHED   3 LAPS   BEST 29.9`
once it is over, **N** starts a new race, and the gate to fly through next is drawn green and stronger (the start/finish
gate is yellow while it is not next); the highlight follows the state message, so it changes on the frame the gate is
passed.

## From Python

```python
import ofs

sim = ofs.launch(headless=True)
sim.load("quads/opendrone-5f-freestyle.toml", world="flat", mode="lockstep")
print([g.name for g in sim.get_world().gates])           # ['Gate0', 'Gate1', 'Gate2']
state = sim.run(seconds=1.0)
print(state.race.present, state.race.running, state.race.laps)
for event in sim.events():
    if event.kind in ("gate_passed", "lap_completed"):
        print(event.time_s, event.message)

# This flight never moves, so the loop above prints nothing: race events need a steered flight through the gates.
```

Flying the course needs a pilot or a script that can steer: under Betaflight, or on the open-loop stabilizer
(`load(..., open_loop_fc=True, stabilize=True)`). A plain open-loop session cannot hold a path.
`docs/scripted-flight.md` describes a scripted pilot that flies a closed circuit through four gates this way.
