# M3e carried debt

These items were deliberately left out of M3e, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **Font pages 2 and 3.** Betaflight's warnings arrive at page 2 (severity WARNING → `fontSelection[2]`, the
  identity default) and the client falls back to page 0 for them; only page 1 (INFO) draws bold. A page-2
  mapping (or shipping more pages in the atlas) would make warnings bold like a real HD goggle.
- **The DVR has no size mid-recording.** A viewport resize or aspect switch stops the recording instead of
  re-negotiating the header. (With the boxed aspects a plain window resize does not stop it — only a change of
  the picture's internal size does: an aspect switch or a chase toggle.)
- **The DVR has no clean-feed mode.** It records exactly what the pilot sees (the spec's decision 2).
- **A thermal model.** Tramp's 's' status answers a constant 25 °C, like the KISS ESC frames.
- **`FIELD_PERIOD_S` is duplicated** in `ofs-client::fpv` (ofs-video is not a client dependency); the pairing
  with `ofs-video::link::FIELD_RATE_HZ` is by comment only.
- **Junk `fpv_aspect` values silently render boxed 16:9.** The setting is a string pass-through with no
  validation: anything that is not `4:3` or `window` draws the 16:9 box instead of falling back to the 4:3
  default.
- **The DVR's boundary dedup is fp-path-dependent.** Captures dedup on the field boundary
  `render_time_s + held_fields × FIELD_PERIOD_S` compared exactly; after long flights a mid-boundary recovery
  can compute a boundary one ulp off and stretch the AVI by one 20 ms frame.
- **`test_sitl_link.py` was never in the sitl job's pytest list.** Pre-existing, observed during this
  milestone: CI's sitl job names five live Python tests (hover, radio, telemetry, tramp, video) and the ELRS
  link one has never been among them.
- **The spec's own out-of-scope list:** digital video systems; a clean-feed DVR; audio; image-sequence and
  ffmpeg export options; the VTX-interference fault (M4); a full HD/digital OSD element set; configurable
  receiver thresholds; reflections off objects.
