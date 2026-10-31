# Best-lap ghost and local records (#133)

Closes #133 (epic #136). No design conversation took place (the brief was to state the calls in the spec), so every
ruling below is the author's call and open to change on review. It builds on #132's courses (PR #151, protocol 15):
a record belongs to a world, a course and a quad, and the ghost races the course the pilot is racing.

## Why

The roadmap says it plainly: "There is no ghost or best-lap replay" (`docs/roadmap.md`, the race paragraph;
`docs/race-timing.md` says the same under "Not included"). The session's `best_lap_s` dies with the session, and
racing yourself is the mode that keeps a solo sim alive. The issue's done-when: the fastest clean lap is captured
automatically from the state stream (it runs at up to 240 Hz), kept per world, course and quad in `user://`; a
translucent ghost quad replays it in the world; best and last lap surface in the menu and the HUD, with a live delta
while racing against the ghost; the capture, the playback and the persistence are tested where they land.

## Goals

1. The client captures every state's pose (position and attitude) while a race runs, and on a session-best lap keeps
   that lap's track.
2. The records persist in the Godot client — `user://records.json`, separate from the settings chain — keyed by
   world, course and quad, holding best and last lap and the best lap's track.
3. A ghost quad (a translucent second instance of the drawn drone) replays the record's best lap in the world:
   chase camera and FPV alike.
4. The HUD names the record best (it is the lap to beat) and a live delta against the ghost; the menu shows the
   best and last lap for the chosen world, course and quad.
5. Tests at every layer that changed: Rust for the capture, GDScript for the records' round trip, the playback and
   the HUD/menu lines.

## Non-goals / anti-goals

- The wire does not grow: no protocol change, no `ofs-sim` change, no Python change, no golden can move. The server
  never knows a ghost exists.
- No settings-chain fork: no new setting, no `OFS_*` variable, no flag, nothing in `user://ofs_client.cfg`. A ghost
  toggle can ride a later issue if pilots ask.
- The models do not change (the ghost is the same drawn quad, translucent); the worlds' physics and the analog video
  chain are untouched — the ghost is world content the goggles see like any object.
- No sharing, exporting or importing of records; no ghost of another pilot; no per-lap history beyond best and last.
- Free flight keeps today's behaviour: no clock, no ghost.

## The calls

1. **The ghost's data source is the client's state stream, captured in `ofs-client`.** The worker's `ingest`
   (`crates/ofs-client/src/worker.rs:85`) already sees every `pb::State` — position, attitude and the `race` block —
   in simulated-time order at up to 240 Hz (`state_rate_hz`, clamped 1..=240 in `client.rs:43`), before any network
   or display smoothing. A recording (MCAP, server-side, opt-in) is not always on and lives where the client cannot
   read it back; a scripted flight (`docs/scripted-flight.md`) flies a path, not the pilot's lap; live snapshots in
   GDScript see the render frame (~60 Hz), not the stream. The capture lands where the 240 Hz stream lands.
2. **The capture moment is detected in the state stream, not from the `lap_completed` event.** Events travel a
   separate stream (`watch_events`) with no ordering guarantee against the pilot link, and the client's `Event`
   carries only the message text. Instead: a state whose `race.laps` rose above the previous state's completed a
   lap; it is the session's best exactly when `last_lap_s == best_lap_s` (the tracker sets `best` on
   `laps == 0 || lap_s < best_lap_s`, `race.rs:213`); its window is `[time_s − last_lap_s, time_s]` — the buffer is
   trimmed to `time_s − current_lap_s` as it fills, so extraction is a copy. The error against the interpolated
   crossing times is at most one state period (~4 ms), invisible in a ghost.
3. **Clean means no collision inside the lap window.** The client notes each `collision` event's simulated time;
   a lap with one anywhere in its window is not captured and does not update the record (a crashed lap is not a
   ghost worth racing). A `gate_hit` does not void a lap: it already earns nothing, and the posts, where the world
   has them, are the punishment. The record's `last_lap_s` updates regardless (it is a fact, not a judgement).
4. **The track is the best lap's poses only, decimated to at most 60 Hz, in Godot's frame.** Poses are the client's
   converted `Sample`s (Godot frame, the frame the ghost draws in). Extraction keeps every k-th pose so the stored
   track holds at most `60 * lap_s` samples — a 35 s lap is ~2 100 poses, ~200 KB of JSON — and playback
   interpolates (position lerp, attitude slerp), the way `StateBuffer::pose_at_sim` does. Times are stored
   normalised to the lap start (first sample 0, last ≈ `lap_s`).
5. **Records persist in `user://records.json`, nested by declared names.** `{"version": 1, "worlds": {<world name>:
   {<course name>: {<quad name>: {best_lap_s, best_at, last_lap_s, last_at, ghost}}}}}` — the names the wire already
   carries (`World.name`, `RaceState.course`, `LoadReply.quad_name` = the quad config's `name`, `server.rs:364`),
   which is what the menu's catalog reads from the files too (`catalog.gd`), so the key survives a moved checkout.
   The unnamed course keys as `""`. One record per key: only the best lap keeps its ghost, and a faster clean lap
   replaces it. A failed parse keeps the file and starts fresh (the `controls.json` precedent); the settings chain
   (`user://ofs_client.cfg` < `OFS_*` < flags) is untouched.
6. **The bridge hands Godot one capture at a time.** `OfsClient.take_lap_capture()` returns a Dictionary —
   `world`, `course`, `lap_s`, `times`/`positions`/`rotations` as `PackedFloat32Array` (xyz, quaternion xyzw),
   `quad` filled from the loaded session — or `{}` when none is waiting, and takes it (test_extension.gd's inert
   pattern). The worker stores the newest untaken capture in `Shared`; a capture nobody takes is replaced by a
   newer one (only the best matters).
7. **The ghost is a translucent second instance of the drawn quad, driven by the lap clock.** `ghost.gd` extends
   `drone.gd` (same boxes; #130's real model will follow for free), walks its meshes translucent white and spins
   the props at a fixed idle. It lives under `FpvViewport` beside the drone, so the chase camera and the FPV camera
   — and the DVR, and the analog chain after them — see it without a second pipeline. Its pose each frame is the
   track at `race_lap_s` (the pilot's current-lap time: no clock sync needed, both clocks are the lap clock);
   before the clock runs it sits at the track's start, past its finish it parks at the track's end. When #130's model lands the ghost inherits it for free, and the wiring passes the session's quad path to its `set_quad`, so the ghost flies the racer's size rather than the authored 5F's.
8. **The delta is the pilot's lap time minus the ghost's time where the ghost was nearest the pilot.** Each frame,
   a windowed search (±2 s around the last match, which only ever needs to wander) finds the stored pose nearest
   the drone's drawn position; `delta = race_lap_s − ghost_time_at_that_pose`, shown as `−0.8` (ahead) or `+1.2`
   (behind) on the HUD race line while both the race and the ghost run. Gate-by-gate splits stay what they are.
9. **The HUD's BEST is the record's best; the menu shows both numbers.** The HUD line keeps today's format
   (`hud.gd`'s `_update_race`), but `race_best_lap_s` the app hands it is the record best when one exists and is
   faster than the session's (the app already edits the telemetry dictionary for free flight). The delta rides the
   same line's tail. The main menu gains a records line — `Best 29.9 s   Last 31.2 s` — under the Course row when
   the chosen world, course and quad have a record (Course `Default` means the world's first declared course).
10. **The ghost races only a race.** `race_start` off keeps today's free flight: the app already forces
    `race_present` false there (`app.gd`), and the ghost, the delta and the record capture all key on the same
    flag, so the menu-less e2e path and every existing test see nothing new unless a record file exists.

## Testing

Rust (`ofs-client`, a new `lap` module): buffer trim by `current_lap_s`, detection of a best lap (and non-detection
of a slower one), window extraction, the collision veto, the reset/reload clear, the 60 Hz decimation, tracks from
paused and unrunning states. The bridge: inert before a session (test_extension.gd). GDScript: records round trip
(write, read, corrupt file kept), `offer` keeping only a faster clean lap, ghost pose at/around/beyond track times,
the delta ahead/behind/lost-the-ghost, the HUD line with and without a record and delta, the menu line, free flight
and reset hiding the ghost. The e2e suites must pass unchanged (no records file in their `user://`).

## Interaction with open work

PR #169 (terrain, protocol 15 → 16) is open and unmerged: nothing here touches the wire or the world pipeline, so
the two land in either order; the executor re-verifies `PROTOCOL_VERSION` (15 at `ofs-proto/src/lib.rs:12`) and
rebases only for context. The B6 sweep (`b37aae5`) moved `worker.rs`'s `connect_or_launch`, not `ingest`.
