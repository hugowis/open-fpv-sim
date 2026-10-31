# Several courses and race modes per world (#132)

Closes #132 (epic #136). No design conversation took place (the brief was to state the calls in the spec), so every
ruling below is the author's call and open to change on review. #133 (the best-lap ghost) builds on this: it needs to
know which course a lap belongs to, so the course travels in the state, the world and the reset.

## Why

A world declares one course, its `[[gates]]` in file order, and `[race] laps` caps it. One map, one line. The issue
asks for named courses that pick and order the declared gates, a race that names its course in the world file, the
state and the events, a client that picks the course, and one race mode beyond lap counting.

## Goals

1. `[[courses]]` in the world file: a name, an ordered list of declared gate names, an optional mode.
2. The race names its course: `RaceState.course`, the `race_reset` event, `World.courses`, `ResetRace`.
3. The client picks the course: a menu row, **Shift+N** to cycle in the game, the HUD's lap line names it, **N**
   restarts the same course.
4. A first mode beyond laps: **time attack** (the lap-hunt: as many laps as you can fly before the clock runs out).
5. Protocol 15, docs, Python client, tests per course.

## Non-goals / anti-goals

- A world without `[[courses]]` behaves bit-for-bit as before (flat, circuit, anything in the wild); the goldens must
  not move.
- Timing itself (crossing test, splits, laps, hit rules) is unchanged.
- No ghost (#133), no per-course best-time persistence, no slalom (a point-to-point mode would be a second mode; the
  mode field is an enum so it can grow).

## The calls

- **A course picks declared gates; it does not bring its own.** "Picks and orders the declared gates, or brings its
  own": both are the same data (a gate that only one course uses is a `[[gates]]` entry that only that course names),
  so one mechanism covers both. A course lists gate *names*; a gate may appear once per course. The first listed gate
  is the course's start and finish. Gates no course names are not part of any race (Godot does not draw them).
- **`[race] laps` stays the world default** for lap-mode courses; a course's own `laps` overrides it. A time-attack
  course takes no laps (it is the clock that ends the race): `laps` on it is a config error.
- **`[race] course = "Name"`** picks the course a fresh session starts in; without it the first `[[courses]]` entry. A
  world with no `[[courses]]` has one implicit course made of all its gates in file order, with the empty name (so the
  state says `course = ""`, the HUD names nothing, and the events and messages are exactly today's).
- **Gate numbers are course positions.** "Gate1 (gate 1) split ..." and `next_gate` count along the course; with
  courses a gate's number can differ from its place in the file. Godot maps a position to the declared gate through
  `World.courses`.
- **Selecting a course is `ResetRace`.** `ResetRace` takes an optional course name (empty: keep the current course).
  There is no field on `Load`: a client that wants a non-default course resets right after the load (tick 0), which
  the repro journal already replays. The journal line becomes `race_reset` or `race_reset <course index>`.
- **`race_reset` names the course** when the world has named courses: `race reset: Hunt`. Worlds with no named courses
  keep `race reset`. Other messages are unchanged (the state carries the course; a course only changes at a reset).
- **Time attack.** `mode = "time_attack"`, `time_limit_s = 60.0`. The first pass of gate 0 starts the race clock
  *and* the lap clock; laps count as usual; when the race clock reaches the limit the race ends: `race_finished`
  ("race finished: 4 laps", at exactly start + limit, 0 laps is possible), `finished` true, `running` false. The lap
  in progress is discarded. A crossing later than the deadline within the same tick does not count. `best_lap_s` /
  `last_lap_s` stay. The limit is exact (not tick-rounded), like a gate crossing.
- **New signals** (gate worlds only, so other recordings are unchanged): `race.course` (index) and `race.start_s`
  (the race clock's start, time attack needs it).
- **Protocol 15.** `enum RaceMode { RACE_MODE_LAPS = 0; RACE_MODE_TIME_ATTACK = 1; }`, `message Course {name, gates,
  mode, laps, time_limit_s}`, `World.courses` (the declared ones; empty for an implicit course) and
  `World.default_course` (index), `RaceState.course/course_index/mode/time_limit_s/time_left_s`, and
  `ResetRace(ResetRaceRequest{course})`. Python: `ofs.Course`, `Race.course/course_index/mode/time_limit_s/time_left_s`,
  `World.courses`, `sim.reset_race(course="")`. An unknown course name is `InvalidArgument`.
- **Godot.** Menu: a "Course" row when the chosen world declares at least two courses (setting `course`, `--course`,
  `OFS_COURSE`; empty = the world's default); the app resets into it after the session is ready. In the game
  **Shift+N** cycles to the next course and restarts it; **N** restarts the current one (also after a reload and
  across `R`, because the app keeps the pilot's course and re-applies it on `session_ready`). The HUD prefixes the course name
  and, in time attack, shows the time left and the laps. The world draws only the current course's gates, numbered by
  course position.
- **Shipped worlds.** `park` gets two courses: "Diamond" (today's, first, the default) and "Hunt" (the same gates, time
  attack, 60 s), so the menu has something to show. Existing course behaviour of `park` is unchanged.

## Testing

Config (parse, resolve, every error), tracker (per-course lap counting on a shared gate set, time attack including the
exact deadline and the in-tick crossing), session (event text, state), gRPC (reset with a course, unknown course),
journal round trip and replay, Python, Godot (HUD text, catalog scan, menu row, world gate drawing).
