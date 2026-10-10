# M3e: video extras — design

Status: design approved in conversation 2026-10-10, awaiting written-spec review. Parent spec: `2026-10-04-open-fpv-sim-design.md` (§4.2 `ofs-video`, §4.3 Godot client, §9 milestone M3). Predecessor: `2026-10-10-m3c-world-collision-elrs-design.md` (merged as 4b0fb8f), whose milestone table created this one: "M3e: video extras — Tramp, video latency and frame drops, DVR, HD/digital OSD (font page 1), the OSD aspect-ratio setting. Least core; trimmed in its own design conversation." This spec builds on M3d's merged output — protocol 6, `fc_telemetry` — and its implementation plan is written after M3d merges. Carried debt it closes: "Tramp", "Font page 1 and the HD/digital OSD" in `docs/superpowers/m3a-carried-debt.md`; "Video latency, frame drops and DVR" in `docs/superpowers/m3b-carried-debt.md`.

## 1. Purpose and scope

The last of the three M3 follow-ups finishes the video story: the picture behaves like a real analog link in time as well as in quality, the goggles can record it, a second VTX protocol works, and the OSD draws the glyphs Betaflight can address beyond its analog font.

**Success looks like:** flying the Godot client, the picture lags the quad by the configured latency — juddering at the field rate, exactly what analog glass does — and holds (freezes) through a marginal-signal field drop instead of smoothly degrading. The DVR keybind records what the pilot sees; the saved `.avi` plays in VLC showing the degradation, the OSD and the freezes. A Tramp-configured quad responds to Betaflight's VTX commands — channel, power, pit — as the SmartAudio one does. Font-page-1 glyphs draw from the extended atlas, and the picture is a 4:3 box (or 16:9, or window-fill) by setting.

### Decisions made in the design conversation

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | **Server-authoritative timing**: the sim models latency and field drops in simulated time; the client composites from delayed, field-quantized samples. | The client delays its own state locally (headless users would measure undelayed signals — the sim's data would disagree with the pilot's experience, and drops would be invisible to tests, logs and determinism). |
| 2 | **DVR records the degraded composite** — the FPV view as the pilot sees it: degradation, OSD, freezes. | A clean feed (what real goggle DVR captures is the degraded link; a clean recording is a different feature). |
| 3 | **MJPEG AVI written by our own muxer** (Rust, in `ofs-client`), JPEG via Godot's built-in encoder. No external dependency. | An image sequence plus a manual ffmpeg step (the deliverable is a folder, not a file); piping frames to ffmpeg for H.264 (an external binary on PATH, process lifecycle to manage). |
| 4 | **The FPV composite moves into a fixed-resolution SubViewport** with a configurable aspect, boxed in the window. This is what makes DVR honest (it captures the picture without the sim's HUD) and what the aspect setting sizes. | Keep rendering full-window and capture a region (the HUD overlaps the picture; the aspect setting would re-implement the boxing per layer). |
| 5 | **Field drops live only in the marginal band**: probability ramps from 0 at `UNSTABLE_SNR_DB` (6 dB) to a named constant (~0.5) at `LOST_SNR_DB` (3 dB). | Drops across the whole SNR range (a locked link never drops fields; a lost link is already static — drops there would be invisible or wrong). |

### Out of scope

Digital video systems (HDZero, OpenHD/Ruby); a DVR of the clean feed; audio; image-sequence and ffmpeg export options; the VTX-interference fault (M4); a full HD/digital OSD mode (different element set — only font-page drawing lands here); configurable receiver thresholds (M3b debt, stays); reflections off objects; Wi-Fi or other 2.4 GHz interference (not video anyway); non-flat terrain; the M4 fault catalog.

## 2. Video timing in the simulator (`ofs-video::link`)

The link keeps computing each 50 Hz field at **reception** time exactly as today — channel state at the receiver is the physically right instant, so SNR, RSSI, picture and sync are unchanged. Two additions:

- **Field drops.** After computing a field's SNR, a Bernoulli draw from the model's own seeded RNG decides whether the receiver fails to complete the field: the drop probability is 0 at or above `UNSTABLE_SNR_DB`, ramps linearly to `FIELD_DROP_P_MAX` (named constant, ~0.5) at `LOST_SNR_DB`, and is 0 again below `LOST_SNR_DB` (a lost link is static; drops there mean nothing). Published as a new `video.field_valid` signal (1 = the field arrived, 0 = dropped). A dropped field never updates the receiver's `SyncTracker` — the receiver is busy failing, not relocking.
- **Glass-to-glass latency config.** `latency_ms` on the world file's `[receiver]` section (the goggles' properties live there), quantized **down** to whole fields (20 ms at 50 Hz) — a pipeline that buffers fields delivers whole fields. Default ~40 ms = 2 fields (camera + VTX + air + goggles estimate; the provenance note goes into `docs/research/video-link.md`). Exposed to clients in the `VideoLink` state message.

**Determinism:** the only new randomness is the drop Bernoulli, drawn from the link model's existing seeded stream in a fixed order (one draw per field step, before or after the fading draws — the plan pins which and keeps it stable). Lockstep runs stay bit-identical; two runs with the same seed drop the same fields.

## 3. The client's delayed composite (Godot)

One rule decides what the pilot sees at wall-clock frame N: the **render instant** is the newest field boundary at or before `sim_now − latency`. Pose, video quality and OSD are all sampled at that one instant:

- **Pose** from the `StateBuffer` at the deeper delay (`latency +` the remainder of the partial field, so the pose is always at the field boundary). The buffer's 64-sample history is ample; it already renders a configurable delay in the past.
- **Video quality** (noise, sparkles, chroma, sync, `field_valid`) from a small client-side buffer of state samples keyed by `time_s` — the newest sample at or before the render instant.
- **OSD** — the newest `OsdFrame` with `time_s ≤` the render instant (frames already carry `time_s`).

The selection logic lives in **`ofs-client` as a Rust function** (beside `StateBuffer`), unit-tested without Godot; the GDExtension feeds it the streams and gets back what to draw. A dropped field (`field_valid = 0` at the render instant) **holds the previous composite** — pose, OSD and quality all freeze for one field — which is what a real receiver does with an incomplete field. Sync-lost rolling and static are unchanged: those are already the receiver's behaviour, drawn as today.

Because everything samples one shared field-quantized instant, the picture ages as a unit at 50 Hz — the analog judder — and no layer can disagree about "when" it is.

## 4. The FPV composite becomes a SubViewport (the DVR and aspect seam)

Today the 3D scene renders full-window, the degradation shader is full-rect, and the OSD has its own 4:3 box — on a 16:9 window the sides get grain but no OSD, and there is nothing to capture that excludes the HUD. The FPV composite — 3D view, lens, OSD, video effects — moves into a **`SubViewport` at a fixed internal resolution** (independent of window size; the plan pins the number), shown boxed (pillar/letterboxed, centred like today's `box_for`) in the window; the HUD stays the only root-level layer over it. The chase view renders as it does today.

- **The aspect setting** replaces the OSD-only box logic (M3a's "aspect-ratio setting" debt, in a better shape): **4:3** (new default — the goggles-honest shape), **16:9**, or **window** (fill the window, today's behaviour). It sizes the SubViewport; the OSD fills it naturally.
- **Font page 1** rides along: the server already decodes the DisplayPort attribute bits into cells (`ch | page << 8 | blink << 10`, 2 page bits), so `make_osd_font.py` grows a second `.mcm` argument and stacks the pages vertically in the atlas; `osd.gd` offsets the glyph row by the page bits it already receives. Pages beyond what's shipped draw from page 0, defensively. Whether the pinned Betaflight ever *emits* page ≠ 0 is verified during planning (against the pinned checkout, M3a's discipline); if it never does, the drawing support is proven by test-hook injection and the fact is recorded in carried debt.

This restructure is the riskiest change (it touches the whole render path), so it goes **first** in the order of work, with the existing pixel/e2e tests (`shots.gd`, `test_osd.gd`, `test_video.gd`) guarding everything built after it.

## 5. DVR (Godot capture + `ofs-client` muxer)

- **Capture:** the recording runs at the field cadence: after each field boundary whose field is valid, the SubViewport's texture is read back and JPEG-encoded with Godot's built-in encoder; a dropped field contributes no new image, so its slot in the container repeats the held frame — the recording freezes exactly where the pilot froze. Capture runs on a budget: encode and I/O must not stall the frame loop (the plan decides worker/threading, following the client's existing worker pattern); dropped capture frames are acceptable and counted — a recording that hiccups is fine, a flight that stutters is not.
- **Muxer:** a small AVI writer in `crates/ofs-client` — MJPEG in a standard AVI container, pure byte logic, no external dependency, unit-tested against golden headers. Plays in VLC/mpv.
- **UX:** a keybind plus a controls-menu toggle arms recording; a timestamped `.avi` per recording; HUD toasts mark start/stop; the file lands in the client's user-data directory (`user://recordings/`). Recording exactly reproduces the pilot's view: degraded, with OSD, drops as frozen frames.

## 6. Tramp (`ofs-video::tramp`)

A Tramp codec behind the existing `VtxModel`, selected by the `[vtx] kind` field M3a reserved (`VtxKind` gains a `Tramp` variant; today only `Smartaudio` exists). The codec owns the wire: framing, CRC16, command parsing and response composition; the model keeps its shared state (band/channel/frequency, power, pit) and its bus signals. Tramp's quirks live inside the codec — power in mW natively (no dBm table), pit mode as its own command, frequency set directly in MHz. Byte-level facts are pinned during planning against Betaflight's `vtx_tramp.c` in the pinned checkout.

A second quad config exercises it live: `quads/opendrone-5f-tramp.toml` plus a `.betaflight.diff` enabling the Tramp peripheral on its UART. The shipped quad stays SmartAudio. Config validation accepts the new kind and still rejects unknown ones.

## 7. Protocol and clients

**Protocol 7** (`PROTOCOL_VERSION = 7`; M3d ships 6; all clients move together): the `VideoLink` message gains `field_valid` (bool) and `latency_ms` (uint32). No other wire changes.

**Python and `ofs-client`:** `state.video.field_valid`, `state.video.latency_ms`.

**Godot:** the delayed composite (§3), the SubViewport and aspect setting (§4), DVR (§5). The HUD's link line is unchanged.

## 8. Order of work, failure handling and tests

**Order, riskiest first:**
1. The SubViewport restructure (the render path; guarded by the existing pixel/e2e tests).
2. The timing pipeline: server drops + latency config, then the client's delayed composite.
3. DVR capture + the AVI muxer.
4. Tramp.
5. Font page 1 + the aspect setting polish.
6. Docs (`dev-setup.md`, the `video-link.md` research note, `README.md`, `m3e-carried-debt.md`) and CI (the sitl job gains the Tramp test).

**Failure handling:** a DVR encode or disk-write failure stops the recording with a toast (a failed recording is simulator behaviour for the pilot, an error for the logs — never a crash); the muxer tolerates a mid-recording client shutdown by finalizing what it has. A malformed Tramp frame is counted, never a panic (the SmartAudio parser's contract). A `latency_ms` of 0 or missing `[receiver]` defaults behave sensibly (0 fields of latency; defaults as today).

**Tests:**
- **Unit (`ofs-video`):** the Tramp codec — golden frames, CRC, malformed-input fuzz (`proptest`, the M1 rule); the drop band — no drops at or above 6 dB or below 3 dB, probability inside the band, same seed ⇒ same drops; latency quantization (37 ms → 1 field, 40 ms → 2).
- **Unit (`ofs-client`):** the AVI muxer against golden bytes; the delayed-composite selection — field-boundary pick, hold-on-drop, newest-OSD-at-or-before.
- **Font script:** the atlas stacks two pages; `osd.gd` draws page-1 cells from the second block (Godot test) and falls back to page 0 beyond it.
- **Simulator (lockstep):** two runs with latency + drops configured are bit-identical; `field_valid` only drops inside the band; `latency_ms` reaches the state snapshot.
- **Live SITL (`python/tests/`):** the Tramp quad — Betaflight's channel, power and pit commands reach the simulated VTX (the `vtx.*` signals follow) and the video frequency changes with them.
- **Godot e2e:** DVR produces a parseable `.avi` whose frame count matches the recording duration at ~50 fps and whose frames decode as JPEG; the aspect setting resizes the box; the composite renders from the delayed field-quantized instant (asserted through a test hook).
- **Manual check:** fly with the default latency (the judder is visible but not annoying), record a DVR clip and play it in VLC, flip the aspect setting through all three values, change channel and power on the Tramp quad from the OSD menu.

## 9. Risks

| Risk | Mitigation |
|---|---|
| The SubViewport restructure regresses the render path. | It goes first; `shots.gd` pixel comparisons and the e2e suite guard it before anything else lands. |
| The pinned Betaflight never emits font page ≠ 0, so page-1 drawing can't be proven live. | Verified during planning; if so, the drawing is proven by test-hook injection and the gap is recorded in `m3e-carried-debt.md`. |
| DVR encode/IO stalls the flight loop. | Budgeted worker; dropped captures are counted and acceptable; the plan benchmarks at the SubViewport's fixed resolution. |
| The 40 ms default latency makes flying feel worse. | It is configurable (0 allowed) and quantized to 2 fields by default; the manual check flies with it before it ships. |
| Tramp byte-level facts differ from memory (no pinning yet). | Pinned during planning against the pinned checkout, M0/M3a discipline; the codec follows the facts, not the reverse. |
| Client video-sample buffer adds frame cost. | The selection logic is Rust beside `StateBuffer`; the buffer is bounded (fields, not frames). |
