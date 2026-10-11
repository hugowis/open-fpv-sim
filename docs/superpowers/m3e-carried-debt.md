# M3e carried debt

These items were deliberately left out of M3e, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **Font pages 2-3.** The atlas now ships two pages — 0 (default) and 1 (bold, downloaded after the maintainer
  approved the M3a font discipline) — and `osd.gd` draws the font-page bits the simulator packs, so Betaflight's
  INFO elements draw bold like a real HD goggle. Warnings (page 2) and criticals (page 3) still fall back to
  page 0; shipping those pages (or a mapping) is the remainder.
- **The DVR has no size mid-recording.** A viewport resize or aspect switch stops the recording instead of
  re-negotiating the header. (The recording stops when the picture's internal size changes: an aspect switch, a
  chase toggle from a boxed aspect, or a window resize with the window aspect — with the boxed aspects a plain
  window resize does not stop it.)
- **The DVR has no clean-feed mode.** It records exactly what the pilot sees (the spec's decision 2).
- **A thermal model.** Tramp's 's' status answers a constant 25 °C, like the KISS ESC frames.
- **`FIELD_PERIOD_S` is duplicated** in `ofs-client::fpv` (ofs-video is not a client dependency); the pairing
  with `ofs-video::link::FIELD_RATE_HZ` is by comment only.
- **The DVR's boundary dedup is fp-path-dependent.** Captures dedup on the field boundary
  `render_time_s + held_fields × FIELD_PERIOD_S` compared exactly; after long flights a mid-boundary recovery
  can compute a boundary one ulp off and stretch the AVI by one 20 ms frame.
- **`test_sitl_link.py` was never in the sitl job's pytest list.** Pre-existing, observed during this
  milestone: CI's sitl job names five live Python tests (hover, radio, telemetry, tramp, video) and the ELRS
  link one has never been among them.
- **The spec's own out-of-scope list:** digital video systems; a clean-feed DVR; audio; image-sequence and
  ffmpeg export options; the VTX-interference fault (M4); a full HD/digital OSD element set; configurable
  receiver thresholds; reflections off objects.

## Surfaced by the final review
- **The spec's controls-menu DVR toggle shipped as keybind-only** (spec §5: "a keybind plus a controls-menu
  toggle"). Recorded deviation: the D keybind plus its HUD help line is the shipped UX; a toggle on the F2
  screen would close it.
- **The spec's two listed tests were not delivered.** The Godot e2e's DVR check (a parseable `.avi` whose frame
  count matches the recording duration at ~50 fps and whose frames decode as JPEG) and the simulator lockstep
  run of two bit-identical sessions with latency + drops configured (the model level is covered: the Python
  same-seed lockstep test). A headless JPEG-path pin — `Image.create` + `save_jpg_to_buffer` through
  `dvr_write_frame` — would close the DVR's JPEG-quality guard beyond the comment in `godot/ui/dvr.gd`.
- **A same-name DVR start still truncates silently.** The filename's timestamp now carries a millisecond
  uniqueness suffix, so start/stop/start within one second no longer collides — but a genuine same-name start
  (a clock without millisecond resolution, a scripted caller) opens with `File::create` and truncates the
  previous recording instead of the start failing.
- **`VideoHandles::read` reports latency 0 without a VTX** (`crates/ofs-sim/src/vehicle.rs`): `VideoInfo::default()`
  zeroes it when `present` is false, though the goggles' glass-to-glass latency is arguably not VTX-conditional.
