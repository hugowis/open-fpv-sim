# DVR bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A DVR that survives a resize (#34), can record a clean camera feed (#35), and exports an image sequence and an H.264 MP4 (#37).

**Architecture:** The Rust client gains named DVR feeds (two independent writers) and a `dvr_export` module that post-processes a finished AVI (frame copy, optional ffmpeg). The Godot `dvr.gd` fits every captured image to the recording's starting size, drives one or two feeds, and a new `ui/clean_feed.gd` renders the camera image (3D + lens) in a second SubViewport that only updates while a clean recording runs.

**Tech Stack:** Rust (`ofs-client`, `ofs-godot`), GDScript (Godot 4.7.2), `ffmpeg` on `PATH` as an optional runtime tool.

**Spec:** `docs/superpowers/specs/2026-10-25-dvr-bundle-design.md`

## Global Constraints

- A DVR start never overwrites a file; frames are keyed on the integer `target_field`.
- No new mandatory dependency; ffmpeg is optional and looked up on `PATH`.
- The live format stays MJPEG AVI at `DVR_FPS` = 50.
- No protocol change, no server change.
- Rust gate: `cargo test --workspace --locked` and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Godot gate: `GODOT_BIN=<godot 4.7.2> bash scripts/run-godot-tests.sh unit`, then `... open-loop` (WSL/Linux; see `CONTRIBUTING.md`). Do not run Windows and WSL cargo at once (shared `target/`).
- Commit messages `type(scope): what`; one PR, one `Closes #n` per line (#34, #35, #37).
- Write GDScript/Rust files with Edit/Write, never a Bash heredoc (CRLF and backslash gotchas).

## Review Focus

- A crashed recording (AVI header sizes still 0) must still export its frames (Task 3).
- A resize to a wider, taller, or odd-sized picture mid-recording keeps the file's declared size and every frame decodable (Task 2).
- A held field repeats the previous frame in **both** feeds so the two files have equal frame counts (Task 6).
- ffmpeg absent, or exiting non-zero, leaves the AVI untouched and reports instead of crashing (Task 3, Task 4).
- An export target that already exists is an error, never an overwrite (Task 3).

---

### Task 1: Named DVR feeds in the client

**Files:**
- Modify: `crates/ofs-client/src/dvr.rs` (add `DvrFeed`)
- Modify: `crates/ofs-client/src/client.rs:35,92,190-211` (the `dvr` field and the three methods)
- Modify: `crates/ofs-godot/src/lib.rs:312-340` (`dvr_start`, `dvr_write_frame`, `dvr_stop`)
- Modify: `godot/ui/dvr.gd` (pass `"goggles"`), `godot/tests/test_dvr.gd` (mock signatures, direct client calls)
- Test: `crates/ofs-client/tests/dvr.rs`, `godot/tests/test_dvr.gd`

**Interfaces:**
- Produces (Rust):
  `pub enum DvrFeed { Goggles, Clean }` with `DvrFeed::parse(&str) -> Option<DvrFeed>` (`"goggles"`, `"clean"`) in `ofs_client::dvr`;
  `Client::dvr_start(&self, feed: DvrFeed, path: String, width: u32, height: u32) -> Result<(), String>`;
  `Client::dvr_write_frame(&self, feed: DvrFeed, jpeg: Vec<u8>)`;
  `Client::dvr_stop(&self, feed: DvrFeed) -> Result<DvrSummary, String>`. Each feed has its own writer; starting a feed that already records returns `Err("already recording")`.
- Produces (GDScript, `OfsClient`): `dvr_start(feed: String, path: String, width: int, height: int) -> String` ("" or why not; an unknown feed name is an error string), `dvr_write_frame(feed: String, jpeg: PackedByteArray)`, `dvr_stop(feed: String) -> Dictionary`.

- [ ] **Step 1: Write the failing Rust test** in `crates/ofs-client/tests/dvr.rs`: `dvr_feed_parse_knows_goggles_and_clean_only` asserts `DvrFeed::parse("goggles") == Some(DvrFeed::Goggles)`, `parse("clean") == Some(DvrFeed::Clean)`, `parse("Clean")` and `parse("")` are `None`. (`Client` needs a session, so the per-feed independence is covered by the Godot test in Step 5.)
- [ ] **Step 2:** `cargo test -p ofs-client --test dvr dvr_feed` fails to compile (`DvrFeed` undefined).
- [ ] **Step 3: Implement** `DvrFeed` (derive `Debug, Clone, Copy, PartialEq, Eq`) in `dvr.rs`; change `Client.dvr` to `Mutex<[Option<DvrWriter>; 2]>` indexed by feed and the three methods to the signatures above. Update the `ofs-godot` bindings to parse the feed name first (unknown name: `dvr_start` returns `"unknown feed <name>"`, `dvr_write_frame` ignores, `dvr_stop` returns an empty Dictionary).
- [ ] **Step 4:** `cargo test -p ofs-client --test dvr` passes; `cargo clippy -p ofs-client -p ofs-godot --all-targets --locked -- -D warnings` is clean.
- [ ] **Step 5: Godot tests.** In `test_dvr.gd` update the mock (`dvr_start(_feed, _path, _width, _height)`, `dvr_write_frame(_feed, jpeg)`, `dvr_stop(_feed)`) and the direct client tests; add `test_two_feeds_record_independently`: with a real session client, `dvr_start("goggles", p1, 4, 2)` and `dvr_start("clean", p2, 4, 2)` both return `""`, a second `dvr_start("clean", ...)` is non-empty, writing 2 frames to goggles and 3 to clean then stopping each gives `frames` 2 and 3, and `dvr_start("bogus", ...)` is non-empty. Change `dvr.gd` to pass `"goggles"`.
- [ ] **Step 6:** `bash scripts/run-godot-tests.sh unit` passes.
- [ ] **Step 7: Commit** `feat(dvr): named feeds in the client and the Godot binding`.

---

### Task 2: A DVR that survives a resize (#34)

**Files:**
- Modify: `godot/ui/dvr.gd` (`_size`, `fit_image`, capture)
- Modify: `godot/scripts/app.gd:106-118,205-211` (`_layout_fpv`, `set_chase_view`)
- Modify: `godot/tests/test_dvr.gd` (the resize test at the "picture size change stops the DVR" check), `godot/tests/e2e_open_loop.gd` only if it asserts the old stop
- Test: `godot/tests/test_dvr.gd`

**Interfaces:**
- Produces: `static func fit_image(image: Image, width: int, height: int) -> Image` in `dvr.gd`: returns `image` itself when it already has that size; otherwise a new `width`×`height` image, black, with `image` scaled to fit inside (aspect preserved, bilinear) and centred. Result has the same format as the input.

- [ ] **Step 1: Write failing tests** in `test_dvr.gd`:
  - `test_fit_image_keeps_a_same_size_image_untouched` (`fit_image(img, 8, 4) == img`, same object).
  - `test_fit_image_letterboxes_a_wider_image`: a 16×4 solid red image fitted to 8×8 gives an 8×8 image whose row 0 is black, whose centre pixel is red, and whose last row is black. `test_fit_image_pillarboxes_a_taller_image`: 4×16 red into 8×8: column 0 black, centre red.
  - `test_a_resize_mid_recording_keeps_the_declared_size`: start against the mock with a 8×4 viewport, set `image_source` to return a 6×6 image then a 16×16 image; every JPEG the mock receives decodes (`Image.load_jpg_from_buffer`) to 8×4.
  - Rewrite the existing `test_the_menu_d_and_a_resize_keep_the_dvr_views_in_sync` check "a picture size change stops the DVR" (lines ~170-175): after `app._layout_fpv()` with a different size the DVR **still records** and the menu stays checked; add `test_entering_the_chase_view_stops_the_dvr` (`app.set_chase_view(true)` while recording: not recording, menu unchecked, a toast containing "chase").
- [ ] **Step 2:** `bash scripts/run-godot-tests.sh unit` fails on the new/changed tests.
- [ ] **Step 3: Implement** `fit_image`; store the starting size in `start` (`_size = viewport.size`); in `capture` pass the image through `fit_image(image, _size.x, _size.y)` before `save_jpg_to_buffer`. In `app.gd` delete the stop-on-size-change block from `_layout_fpv`; in `set_chase_view(true)` stop a running recording with the toast "DVR stopped: the chase camera is not recorded (N frames saved)" (same stop/sync path as `_set_dvr(false)`). Update `docs` text later in Task 7.
- [ ] **Step 4:** `bash scripts/run-godot-tests.sh unit` and `... open-loop` pass.
- [ ] **Step 5: Commit** `feat(dvr): a resize or aspect switch no longer stops a recording`.

---

### Task 3: The export module (#37 core)

**Files:**
- Create: `crates/ofs-client/src/dvr_export.rs`
- Modify: `crates/ofs-client/src/lib.rs` (add `pub mod dvr_export;`)
- Test: `crates/ofs-client/tests/dvr_export.rs`

**Interfaces:**
- Produces (all in `ofs_client::dvr_export`):
  - `pub fn export_frames(avi: &Path) -> Result<(PathBuf, u64), String>`: writes the AVI's `00dc` chunks, unmodified, to `<avi stem>_frames/frame_%06d.jpg` (1-based) next to the AVI, returning the directory and the count. The directory must not exist (`Err` naming it otherwise); every file is created with `File::create_new`. Chunks are found by scanning from the `movi` fourcc until `idx1` or end of file, ignoring header sizes; a chunk cut off by end of file is dropped.
  - `pub fn find_ffmpeg_in(path_var: &OsStr) -> Option<PathBuf>` (first `ffmpeg`/`ffmpeg.exe` file in the `PATH`-style list) and `pub fn find_ffmpeg() -> Option<PathBuf>` (reads `PATH`).
  - `pub fn ffmpeg_args(avi: &Path, mp4: &Path) -> Vec<OsString>`: exactly the argument list in spec R3, starting at `-nostdin`.
  - `pub fn export_mp4(ffmpeg: &Path, avi: &Path) -> Result<PathBuf, String>`: runs `ffmpeg_args(avi, <stem>.mp4)`; `Ok(mp4 path)` on exit 0; otherwise `Err` carrying the last non-empty line of stderr (or the spawn error). Fails before spawning when the mp4 already exists.

- [ ] **Step 1: Write failing tests** in `tests/dvr_export.rs` (build AVIs with `DvrWriter`, junk bytes as frames, in a per-test temp dir as `dvr.rs` tests do):
  - `frames_are_copied_verbatim_and_numbered_from_one`: frames `b"AAA"`, `b"BBBBB"` (odd size, padded in the AVI) export to `frame_000001.jpg == AAA`, `frame_000002.jpg == BBBBB`, count 2, dir `<stem>_frames`.
  - `an_unfinalized_avi_still_exports`: write 3 frames, then copy the file with its header size fields zeroed (bytes 4..8, the `movi` size at 216, avih/strh frame counts at 48 and 140 — the offsets `dvr.rs` documents) and truncate the idx1: export yields 3 frames; truncate mid-chunk: the partial last chunk is dropped.
  - `an_existing_frames_directory_is_an_error_and_untouched`.
  - `a_file_that_is_not_an_avi_is_an_error` (random bytes: `Err`, no directory created).
  - `ffmpeg_args_are_the_documented_list` (asserts the `-n`, `-nostdin`, the `pad=ceil(iw/2)*2:ceil(ih/2)*2` filter, `libx264`, `yuv420p`, `-crf 18`, `+faststart` and the output last).
  - `find_ffmpeg_in_finds_the_first_match_and_none_when_absent` (a temp dir holding an empty file named `ffmpeg` or `ffmpeg.exe` by `cfg!(windows)`).
  - `export_mp4_with_a_missing_program_is_an_error_and_leaves_the_avi` (`Path::new("no-such-ffmpeg")`).
  - `export_mp4_refuses_an_existing_mp4`.
- [ ] **Step 2:** `cargo test -p ofs-client --test dvr_export` fails to compile.
- [ ] **Step 3: Implement** the four functions. `export_frames` scanning: find `movi` (first occurrence at or after offset 12), then loop reading `tag(4) size(4)` until the tag is `idx1` or fewer than 8 bytes remain; skip the pad byte on odd sizes. Validate the file starts `RIFF....AVI ` first.
- [ ] **Step 4:** `cargo test -p ofs-client` passes; `cargo clippy -p ofs-client --all-targets --locked -- -D warnings` clean.
- [ ] **Step 5: Commit** `feat(client): export a DVR recording as an image sequence or, through ffmpeg, an MP4`.

---

### Task 4: Export in the game: binding, setting, button, toasts (#37)

**Files:**
- Modify: `crates/ofs-client/src/dvr_export.rs` (add the job runner), `crates/ofs-godot/src/lib.rs` (two `#[func]`s)
- Modify: `godot/scripts/settings.gd` (defaults, `ENV_NAMES`, `FLAG_NAMES`), `godot/ui/controls_menu.gd`, `godot/scripts/app.gd`
- Test: `crates/ofs-client/tests/dvr_export.rs`, `godot/tests/test_settings.gd`, `godot/tests/test_controls_menu.gd`, `godot/tests/test_dvr.gd`

**Interfaces:**
- Consumes: Task 3's `export_frames`, `find_ffmpeg`, `export_mp4`.
- Produces (Rust): `pub struct Exporter` with `Exporter::new() -> Exporter`, `Exporter::spawn(&self, avi: PathBuf, frames: bool, mp4: bool)` (one worker thread per call, runs frames then mp4) and `Exporter::poll(&self) -> Vec<ExportDone>`, where `pub struct ExportDone { pub avi: PathBuf, pub kind: &'static str /* "frames" | "mp4" */, pub result: Result<PathBuf, String> }`. With `mp4` requested and no ffmpeg found, the job reports `kind: "mp4"`, `Err("ffmpeg was not found on PATH")`.
- Produces (GDScript, `OfsClient`): `dvr_export(path: String, mode: String) -> String` ("" started; or why not — mode must be `frames`, `mp4` or `both`) and `dvr_export_poll() -> Array` of `{path, kind, ok, detail}` Dictionaries (`detail` is the output path or the error). Settings key `dvr_export` (default `"none"`, valid `none|frames|mp4|both`); `Settings` coerces an unknown value to the default. `controls_menu.gd`: `signal dvr_export_changed(value: String)`, `set_dvr_export(value: String)`, a button "DVR export: none" cycling `none → frames → mp4 → both`, built like the "Picture" button.

- [ ] **Step 1: Write failing tests.** Rust: `the_exporter_reports_a_frames_job_and_a_missing_ffmpeg` (spawn with `frames: true, mp4: true` on a written AVI with `PATH` unset-safe: call `Exporter::spawn_with(ffmpeg: Option<PathBuf>, ...)` internally so the test passes `None`; poll until two `ExportDone` arrive within 5 s: one frames `Ok`, one mp4 `Err` containing "ffmpeg"). Godot: `test_settings.gd` — default `dvr_export == "none"`, `OFS_DVR_EXPORT=mp4` and `--dvr-export=both` resolve, `--dvr-export=bogus` falls back to `none`; `test_controls_menu.gd` — the button cycles in the order above, emits `dvr_export_changed`, and `set_dvr_export("mp4")` shows it without emitting; `test_dvr.gd` — an app-level test with a mock client whose `dvr_export_poll()` returns one ok and one failed result: `app._poll_dvr_exports()` toasts "exported" for the first and a `warn` toast containing the error for the second.
- [ ] **Step 2:** all of them fail.
- [ ] **Step 3: Implement.** `Exporter` holds an `mpsc` channel; `spawn_with` is the testable core, `spawn` calls it with `find_ffmpeg()`. Bind the two `#[func]`s on `OfsClient` (the `Exporter` is a field of the node, independent of any session). In `app.gd`: after a successful stop (`_set_dvr(false)` and the chase stop from Task 2) call `client.dvr_export(<each recorded path>, settings["dvr_export"])` when the setting is not `none`; call `_poll_dvr_exports()` from `_process`; wire the menu button like `aspect_changed` (apply and `AppSettings.save_value("dvr_export", value)`, toast on save failure). Validation of the setting value goes in `settings.gd`'s `resolve` result handling next to `clamp_aspect` usage in `app.gd`.
- [ ] **Step 4:** `cargo test -p ofs-client`, clippy, then `bash scripts/run-godot-tests.sh unit` pass.
- [ ] **Step 5: Commit** `feat(dvr): optional image-sequence and MP4 export after a recording`.

---

### Task 5: The clean-feed viewport (#35 render)

**Files:**
- Create: `godot/ui/clean_feed.gd`
- Modify: `godot/main.tscn` (add the node), `godot/scripts/app.gd` (`@onready`, per-frame sync)
- Test: `godot/tests/test_clean_feed.gd` (new), `godot/tests/run_tests.gd` (register it)

**Interfaces:**
- Produces: a `Node` named `CleanFeed` (script `clean_feed.gd`) under `Main`, owning a `SubViewport` child created in code:
  - `func setup(source_camera: Camera3D, source_lens: CanvasLayer, size: Vector2i) -> void` builds the viewport (same `msaa_3d` as `FpvViewport`, `own_world_3d` false so it shares the world), a `Camera3D` and a `lens.gd` layer, copies the lens's `distortion`/`exposure`/`vignette`.
  - `func set_active(on: bool) -> void`: `render_target_update_mode` = `UPDATE_ALWAYS` when on, `UPDATE_DISABLED` when off; off at start.
  - `func sync_camera() -> void`: copies `global_transform`, `fov`, `near`, `far`, `keep_aspect`, `cull_mask` from the source camera to the clean camera; called by `app.gd` every frame while active.
  - `func resize(size: Vector2i) -> void`: sets the viewport size (called wherever `_layout_fpv` sets `fpv_viewport.size`).
  - `func viewport() -> SubViewport`.

- [ ] **Step 1: Write failing tests** in `test_clean_feed.gd`: `test_it_is_off_until_activated` (mode is `UPDATE_DISABLED` after `setup`, `UPDATE_ALWAYS` after `set_active(true)`, back after `false`); `test_the_camera_follows_the_source` (move/rotate the source camera, change `fov`, call `sync_camera`, compare `global_transform` and `fov`); `test_the_lens_settings_are_copied`; `test_resize_resizes_the_viewport`; `test_the_clean_viewport_has_no_osd_or_video_layer` (no child of the viewport has `osd.gd` or `video.gd` as its script).
- [ ] **Step 2:** `bash scripts/run-godot-tests.sh unit` fails (file missing).
- [ ] **Step 3: Implement** `clean_feed.gd`; add the node to `main.tscn`; in `app.gd` `_ready` call `clean_feed.setup(fpv_camera, lens, fpv_viewport.size)`, in `_layout_fpv` call `clean_feed.resize(internal)`, and in `_process` (FPV branch) call `clean_feed.sync_camera()` when `clean_feed.is_active()` (add that getter).
- [ ] **Step 4:** unit and `open-loop` suites pass.
- [ ] **Step 5: Manual check (real renderer, cannot be automated):** extend `godot/tests/shots.gd` to save `clean_fpv.png` from `clean_feed.viewport().get_texture().get_image()` after `set_active(true)` and a few frames, next to `osd_fpv.png`; run `godot --path godot -s res://tests/shots.gd -- --out=<dir>` and confirm by eye: same view, lens vignette present, no OSD text, no grain even with `video_grain.png`'s conditions. If the clean viewport renders an empty world, the viewport does not share the world: set `world_3d = fpv_viewport.world_3d` explicitly in `setup`.
- [ ] **Step 6: Commit** `feat(dvr): a clean-feed viewport that renders only while it is wanted`.

---

### Task 6: Record the clean feed (#35)

**Files:**
- Modify: `godot/ui/dvr.gd` (two feeds), `godot/scripts/app.gd` (`_set_dvr`, `_process`, settings, menu wiring), `godot/scripts/settings.gd`, `godot/ui/controls_menu.gd`
- Modify: `godot/tests/test_dvr.gd`, `godot/tests/test_settings.gd`, `godot/tests/test_controls_menu.gd`, `godot/tests/e2e_open_loop.gd`

**Interfaces:**
- Consumes: Task 1 feed bindings, Task 2 `fit_image`, Task 5 `CleanFeed`.
- Produces: `dvr.gd` `func start(client: Object, viewport: SubViewport, feed: String = "goggles", clean_viewport: SubViewport = null) -> String` where `feed` is `goggles`, `clean` or `both`; `clean`/`both` require `clean_viewport` (else returns an error string). Paths: `user://recordings/REC_<ts>.avi` and `REC_<ts>_clean.avi` (same `<ts>`); `var paths: PackedStringArray` (globalized, in start order: goggles first). `started(path)` is emitted once with the first path. `clean_image_source: Callable` mirrors `image_source` for tests. `stop()` returns the first feed's summary plus `"paths"`; it returns `{}` if any feed failed to finalize, after stopping all of them. `capture(view)` handles both feeds on the same `target_field`; a held field writes each feed's own last JPEG. Settings key `dvr_feed` (default `"goggles"`, valid `goggles|clean|both`), menu button "DVR feed: goggles" cycling in that order, `signal dvr_feed_changed(value: String)`, `set_dvr_feed(value)`.

- [ ] **Step 1: Write failing tests** in `test_dvr.gd` (mock client records `(feed, bytes)` pairs): `test_both_feeds_start_two_files_with_one_stem` (`paths` has 2 entries differing only by `_clean`); `test_clean_only_starts_one_clean_file`; `test_both_feeds_write_one_frame_each_per_field` (3 distinct `target_field`s → 3 goggles and 3 clean writes; the same `target_field` twice writes once); `test_a_held_field_repeats_both_feeds_last_frames`; `test_a_clean_feed_without_a_clean_viewport_is_refused`; `test_a_feed_that_fails_to_finalize_fails_the_stop_but_stops_the_other`; `test_the_clean_frame_is_fitted_like_the_goggles_frame` (resize case from Task 2 on the clean feed). Settings and menu tests as in Task 4 for `dvr_feed` (`OFS_DVR_FEED`, `--dvr-feed=`, bogus value falls back to `goggles`). In `e2e_open_loop.gd` add a `both` recording of ~1 simulated second using `image_source`/`clean_image_source` synthetic images and assert two files exist, both parse as AVIs with equal `frames`, and `started` fired once.
- [ ] **Step 2:** unit suite fails on the new tests.
- [ ] **Step 3: Implement.** In `app.gd`, `_set_dvr(true)` passes `settings["dvr_feed"]` and `clean_feed.viewport()`, calls `clean_feed.set_active(true)` for `clean`/`both`; every stop path calls `clean_feed.set_active(false)`; `dvr.capture(fpv)` is unchanged. Export (Task 4) iterates `summary["paths"]`.
- [ ] **Step 4:** `bash scripts/run-godot-tests.sh unit` and `... open-loop` pass.
- [ ] **Step 5: Manual check (needs a display):** run the game with `--dvr-feed=both --dvr-export=both`, fly ~10 s with the OSD visible, press D. Open both AVIs: the goggles file has OSD and breakup, the clean file has neither, same duration. Drag the window edge during a second recording: it keeps recording, with black bars.
- [ ] **Step 6: Commit** `feat(dvr): a clean-feed recording, alone or beside the goggles file`.

---

### Task 7: Docs, changelog, full gate

**Files:**
- Modify: `docs/dev-setup.md:172-183` (the DVR paragraph), `docs/roadmap.md:52,142` and `:208` neighbourhood, `CHANGELOG.md` (new "Unreleased" block above 0.4.0), `README.md` only if it lists settings
- Test: whole workspace

- [ ] **Step 1:** Rewrite the dev-setup DVR paragraph: resize no longer stops it (letterboxed to the starting size), the chase view still does; the `dvr_feed` / `dvr_export` settings with their env variables and flags; the file names; ffmpeg lookup and the exact encode settings; export never overwrites. Remove the roadmap limitation "The DVR stops when the picture's size changes… no clean feed" and the backlog line "A DVR that survives a resize; a clean-feed DVR; … image-sequence and ffmpeg export" (leave "audio").
- [ ] **Step 2:** CHANGELOG "Unreleased": `#### Added` clean feed + export settings; `#### Changed` resize no longer stops, **Rust API: `Client::dvr_*` and `OfsClient.dvr_*` take a feed name** (no Python or protocol impact); one line per closed issue.
- [ ] **Step 3: Full gate.** `cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -D warnings` (Windows); in WSL `bash scripts/run-godot-tests.sh all`. Expected: all green (the `sitl_errors` bind test may flake alone; rerun it by itself).
- [ ] **Step 4: Commit** `docs: the DVR bundle (resize, clean feed, export)`; open the PR with one `Closes #34`, `Closes #35`, `Closes #37` per line.
