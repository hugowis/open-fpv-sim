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
height_m = 2.44              # the opening's height; its bottom edge must not be below the ground

[[gates]]
name = "Gate1"
# ...
```

The gates are numbered in file order from 0. **Gate 0 is the start and finish.** A world has at most 64 gates, and a
world without any has no race at all (no model, no `race.*` signals, `State.race.present = false`).

A gate is a *trigger*: it does not collide with the quad and it blocks no signal. Draw its frame with ordinary
`[[objects]]` if you want something to hit: `worlds/flat.toml` has the posts and bar of its three gates as boxes, and
the `[[gates]]` are what those leave free (3.08 m between the posts, 2.44 m under the bar). Godot draws each opening
as a translucent rectangle labelled START or with the gate's number.

## The rules

- **A pass.** Every simulation tick the path from the previous pose to the new one is tested against the *next gate
  only*. It counts when the path goes through the gate's plane in the direction of travel and the point where it does
  lies inside the opening. The test is on the quad's centre: there is no margin for its size, and the gate is
  vertical (no tilt). A path that grazes outside the opening, goes through backwards, or goes through a gate that is
  not next, counts for nothing and changes nothing.
- **Time.** The crossing time is interpolated inside the tick (a quad at 100 m/s crosses a plane between two 8 kHz
  ticks 12 mm apart, and the time is exact to the path's linearity), so it does not depend on the tick rate.
- **The start.** The first pass of gate 0 starts the clock: a `gate_passed` event, split 0.
- **Splits.** Gates 1 to n-1, in order, each raise `gate_passed` with the time since the lap started.
- **A lap.** Passing gate 0 again after the last gate completes a lap (`lap_completed`) and starts the next one. That
  pass raises no `gate_passed`.
- **One gate.** With a single gate every counted pass after the start completes a lap, but only once the quad has been
  farther than 3 m (`REARM_M`) from the gate's centre since the last one, so hovering through the plane does not run up
  laps.
- **Not included.** No lap limit, no reset (a new `Load` starts a fresh race), no ghost or best-lap replay, no
  highlight of the next gate in Godot.

## Events

| kind | message | the event's time |
|---|---|---|
| `gate_passed` | `Gate0 (gate 0) start` for the start, else `Gate1 (gate 1) split 8.412 s` | the crossing |
| `lap_completed` | `lap 1: 31.204 s (best)`; `(best)` when it is the fastest lap so far (the first lap is) | the crossing of gate 0 |

Seconds have three decimals. The format is stable: scripts may parse it.

## State

`State.race` (Python) / `RaceState` (protocol 11):

| field | meaning |
|---|---|
| `present` | the world has gates |
| `running` | gate 0 has started the clock |
| `next_gate` | the gate to fly through next (0 before the start, and again once every gate is passed) |
| `gate_count` | gates in the course |
| `laps` | laps completed |
| `current_lap_s` | time since the current lap started (0 until the clock runs) |
| `last_lap_s`, `best_lap_s` | 0 until a lap completes (`laps > 0` says they are real) |

The recording has the same values as scalar signals `race.running`, `race.next_gate`, `race.laps`, `race.lap_start_s`,
`race.last_lap_s` and `race.best_lap_s`.

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
```

Flying the course needs a pilot or a script that can steer: under Betaflight, or with the open-loop stabilizer when
that lands. An open-loop session without a stabilizer cannot hold a path.
