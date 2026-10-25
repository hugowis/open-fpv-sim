# DVR bundle: resize-proof, clean feed, image-sequence and ffmpeg export (#34, #35, #37)

Closes #34, #35 and #37. One PR, three user-visible changes to the game's DVR (`godot/ui/dvr.gd`,
`crates/ofs-client/src/dvr.rs`). No protocol change, no server change, no new mandatory dependency.

## Why

The DVR records what the pilot sees, but (1) any change of the picture's internal size ends the recording, and in the
default "window" aspect that includes dragging the window edge; (2) the OSD and the analog breakup are burned in, so a
pilot who wants footage to edit has no way to get the camera image; (3) the only output is an MJPEG AVI that most
editors take badly and nothing else can read frame by frame. The brief for the three issues is written in the issues
themselves; the user opted out of a design conversation on 2026-10-25, so every ruling below is the author's call and is
open to change on review.

## Goals

1. **A resize or aspect switch does not end a recording** (#34).
2. **A clean feed** (#35): the camera image without the OSD and without the analog degradation, as its own file or in
   addition to the goggles file.
3. **Export** (#37): after a recording, optionally write an image sequence and/or an H.264 MP4 through `ffmpeg` when it is
   on `PATH`.

## Non-goals

- Audio (#36), a DVR in the chase view, recording from Python or headless runs, an in-game export browser.
- Re-encoding the live recording: the live format stays MJPEG AVI (`DVR_FPS` 50, one `00dc` chunk per field).
- Bundling ffmpeg or linking an encoder library.

## Anti-goals

- A recording must never overwrite a file, and frames stay keyed on the integer field index (`target_field`): both are
  invariants from M3e and the issues repeat them.
- Turning the clean feed on must not cost anything while it is off (no second 3D render).
- A failed export must never lose or alter the AVI it came from.

## Rulings

**R1. Resize (#34): scale to the size the recording started with, letterboxed.** `Dvr.start` fixes the size to
`viewport.size`. Every captured image of a different size is fitted inside that size, aspect preserved, centred on
black (`Dvr.fit_image`). Same size: untouched, no copy. A new segment on resize was rejected: it makes one flight several
files. The chase camera is different, not a size change: `capture` is never called in the chase view, so a recording
would silently skip time. Entering the chase view therefore still stops the recording, with its own toast.

**R2. Clean feed (#35) is the camera image: 3D view plus the lens layer, nothing after it.** The lens (distortion,
exposure, vignette) is optics and stays; the OSD (layer 2) and the analog link layer (layer 3) are goggles and go. A
setting `dvr_feed` takes `goggles` (default, today's behaviour), `clean` or `both`. `both` writes two files with the
same stem, `REC_<ts>.avi` and `REC_<ts>_clean.avi`; `clean` writes only the second. Both feeds are captured on the same
field boundary and a held field (`held_fields > 0`) repeats the previous frame in both, so the two files have the same
frame count and can be laid on one editor timeline. Rendering: a second `SubViewport` that shares the world with
`FpvViewport`, its own `Camera3D` mirroring `FpvCamera` every frame and its own lens layer, and
`render_target_update_mode` `UPDATE_DISABLED` unless a clean recording runs. Restructuring the existing scene so one
render feeds both was rejected: it moves every node path the tests and `shots.gd` rely on.

**R3. Export (#37) is a post-process of the finished AVI, in Rust.** The AVI is the one source of truth, an old recording
exports the same way, and the live path stays untouched. Setting `dvr_export` takes `none` (default), `frames`, `mp4` or
`both`, applied to every file a recording produced.
- *Image sequence:* the AVI's JPEG chunks are copied verbatim (no re-encode) to `<stem>_frames/frame_000001.jpg`,
  `frame_000002.jpg`, …; the directory and every file are created with create-new semantics (an existing one is an
  error). The chunks are found by scanning `movi` rather than trusting the header sizes, so an AVI whose recording
  crashed (header fields still zero) exports too.
- *ffmpeg:* `ffmpeg -nostdin -n -loglevel error -i <avi> -vf pad=ceil(iw/2)*2:ceil(ih/2)*2 -c:v libx264 -pix_fmt yuv420p
  -crf 18 -movflags +faststart <stem>.mp4`. `-n` keeps the never-overwrite rule; the pad keeps odd window sizes legal for
  yuv420p. ffmpeg is looked up on `PATH` (`ffmpeg.exe` on Windows). Absent: the AVI is kept and a warning toast says so.
  Failing: the toast carries the last line of ffmpeg's stderr.
- Exports run on a worker thread; the game polls for results and toasts them.

**R4. Where the choices live.** Two settings in `settings.gd` (`dvr_feed`, `dvr_export`: config file, `OFS_DVR_FEED` /
`OFS_DVR_EXPORT`, `--dvr-feed=` / `--dvr-export=`), two cycle buttons on the F2 screen next to "Picture" (same pattern,
saved at once). An unknown value falls back to the default. The D key and the F2 DVR checkbox are unchanged.

## Constraints

- Windows and Linux/macOS: ffmpeg lookup and the stub-free tests must pass on all three CI runners (no ffmpeg there).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`; the Godot unit suite; the Godot open-loop e2e.
- The Godot `OfsClient` DVR methods change signature (they gain a feed name); the Rust `Client::dvr_*` methods too.
  Pre-1.0 and used only by the game; the CHANGELOG says so.

## Left to the builder

The exact letterbox maths, the camera/lens mirroring details (which `Camera3D` properties to copy), button labels, toast
wording, and the file layout inside `dvr_export.rs`, as long as the tests in the plan pass.

## Known limits (go to the docs and roadmap)

- Letterbox bars appear after a resize; the file keeps the starting size.
- Real-renderer checks (does the clean feed really show no OSD?) cannot run headless: a manual check covers them.
- The H.264 export needs an ffmpeg built with libx264.
