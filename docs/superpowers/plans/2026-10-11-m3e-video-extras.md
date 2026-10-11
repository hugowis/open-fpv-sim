# M3e: video extras — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the video story — the analog picture behaves like real glass in time (latency, field drops), the goggles record what the pilot sees (DVR), a Tramp VTX works beside SmartAudio, and the OSD draws font page 1 in a picture box that follows an aspect setting.

**Architecture:** Server-authoritative timing: `ofs-video::link` decides per 50 Hz field whether it arrived (`video.field_valid`) and the world file carries the glass-to-glass latency; the client composites pose + video quality + OSD at one delayed, field-quantized instant. The FPV composite (3D, lens, OSD, video effects) moves into a fixed-resolution SubViewport, which is what DVR captures (JPEG per field, muxed to MJPEG AVI by `ofs-client::dvr` on a worker thread) and what the aspect setting sizes. Tramp is a second codec behind the existing `VtxModel`/`[vtx] kind` seam.

**Tech Stack:** Rust workspace (ofs-video, ofs-config, ofs-sim, ofs-client, ofs-godot, ofs-proto), Godot 4 (GDExtension via ofs-godot), Python client (grpcio), Betaflight SITL (pinned 2026.6.2, unchanged this milestone).

**Spec:** `docs/superpowers/specs/2026-10-10-m3e-video-extras-design.md` — the plan argues from it; executors read both.

## Global Constraints

- **Protocol 7.** `PROTOCOL_VERSION` goes 6 → 7 in `crates/ofs-proto/src/lib.rs` and the `sim.proto` header comment says so; every client moves together (Task 2).
- **Determinism.** The only new simulator randomness is the field-drop Bernoulli, drawn from `VideoLink`'s existing seeded stream, once per field step, only when transmitting and sync is not Lost. Lockstep runs stay bit-identical.
- **Named constants with provenance.** `FIELD_DROP_P_MAX` (0.5), the default `latency_ms` (40), `FIELD_PERIOD_S` (0.02), `DVR_FPS` (50) and `JPEG_QUALITY` (80) are named constants; their estimates are recorded in `docs/research/video-link.md` (Task 10).
- **Rust style.** Never run bare `rustfmt` (it churns against the repo's compact house style); hand-indent new code. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` after every task that lands Rust — CI's first gate is clippy and local `cargo test` never runs it.
- **Python file edits** (patch-style scripts if any) must open files in binary mode and write LF.
- **Live SITL tests** run inside WSL via `uv run --with grpcio,protobuf,pytest` + a Linux-built `ofs-sim`, `PYTHONPATH=python` (docs/dev-setup.md option 2). The pinned SITL binary is unchanged this milestone — `scripts/build-sitl.sh` and `third_party/betaflight/` are not touched.
- **Godot tests:** `scripts/run-godot-tests.sh` (headless unit suite); e2e: `godot --headless --path godot -s res://tests/e2e_open_loop.gd` (needs `cargo build -p ofs-sim -p ofs-godot`).
- **Commits:** one per task, conventional style (`feat(video): ...`), after the task's tests pass.
- **Licensing:** the second font page is fetched from the official Betaflight Configurator repository **only after the user approves the download in these words** (Task 8, step 1) — the M3a font discipline.

## Rulings and facts verified in advance

The spec pins behaviour; these rulings pin bytes, names and seams, all verified against the merged M3d tree (681267f) and the pinned Betaflight checkout (`~/ofs/betaflight` @ 2026.6.2, e0b7bb01b17b21351057e9ead2d1ab39dd44fa16) while the plan was written. They are the contract where the spec is silent.

1. **Tramp framing (from `src/main/io/vtx_tramp.c` + `tramp_protocol.h`):** frames are 16 bytes `[0x0F][cmd][12-byte payload][crc][0x00]`. The CRC is a u8 **sum of bytes 1..=13** (command + payload, **excluding** the 0x0F — `trampChecksum`, `for (int i = 1; i < 14; i++)`). Note `tramp_protocol.c`'s `trampCrc` sums bytes 0..=13, but `vtx_tramp.c` does not call it — the SITL FC speaks `vtx_tramp.c`, and so does our codec. Requests: `'r'` (0x72) query limits, `'v'` (0x76) query settings, `'F'` (0x46) set frequency (u16 LE MHz), `'P'` (0x50) set power (u16 LE mW), `'I'` (0x49) set active state (u16 LE: **0 = pit mode on, 1 = transmitting** — `trampSendCommand('I', trampConfPitMode ? 0 : 1)`).
2. **Tramp responses (VTX → FC):** the FC's receiver accepts codes `'r'`, `'v'`, `'s'` only (anything else resets its framer), requires byte 15 == 0 and the same checksum. `'r'` payload: min_freq u16 @2, max_freq @4, max_power @6 (accepted only if min_freq ≠ 0). `'v'`: freq @2, configured power @4, control_mode @6, pit_mode @7 (1 = pit on), actual power @8 (accepted only if freq ≠ 0). `'s'`: temperature i16 @6 (accepted only if ≠ 0). **The FC ignores responses to `'F'`/`'P'`/`'I'`** — it re-queries `'v'` ≥ 200 ms later and retries up to 20 times; online it polls `'v'` every ≥ 1 s and asks `'s'` right after each `'v'` reply. So the model answers `'r'`, `'v'`, `'s'` and applies `'F'`/`'P'`/`'I'` silently.
3. **Tramp power table:** with no vtxtable (SITL), the FC uses the driver's built-in `trampPowerTable = {25, 100, 200, 400, 600}` mW. The Tramp quad's `[vtx]` uses exactly those levels.
4. **Tramp has no band/channel mode** (`vtxTrampSetBandAndChannel` is a documented no-op): frequency changes arrive only as `'F'`. The model's existing user-frequency state (`user_freq_mhz`, band/channel reporting 0) matches, as SmartAudio already behaves.
5. **No SITL changes needed:** the patched SITL target already defines `USE_VTX_TRAMP` (`third_party/betaflight/ofs-sitl.patch` line ~957, added with the SmartAudio work) and `vtx_tramp.o` is built. `scripts/build-sitl.sh` is untouched.
6. **Betaflight does emit font pages ≠ 0:** `osd_elements.c` assigns `DISPLAYPORT_SEVERITY_INFO`/`WARNING`/`CRITICAL` attrs (lines 1018/911/805) and `displayport_msp.c` maps `attr → fontSelection[severity]` (identity by default, `displayport_profiles.c:38`). Warnings arrive at **page 2**, some elements at **page 1**. The atlas ships pages 0 + 1 (`default.mcm` + `bold.mcm`, both in the Configurator repo's `resources/osd/2/` — verified via the GitHub API); arriving pages ≥ 2 draw page 0 (the spec's defensive fallback; carried debt notes the page-2 question). Page-1 drawing is provable live (INFO elements) as well as by injection.
7. **The field-drop probability is piecewise:** 0 at SNR ≥ `UNSTABLE_SNR_DB` (6 dB), ramps linearly to `FIELD_DROP_P_MAX` at `LOST_SNR_DB` (3 dB), **0 again below 3 dB** — a lost receiver outputs static, it does not hold frames; the drop draw is also skipped while sync is Lost. (The spec's "0 again below LOST_SNR_DB" constrains the *probability*, not `field_valid`.)
8. **The client's hold-on-drop is a stateless walk-back:** the newest field at or before the render instant that arrived; a dropped field shows the previous one. Bounded by the 64-field buffer (~1.28 s of continuous drops) — beyond it the caller keeps the last picture.
9. **`FIELD_PERIOD_S = 0.02` lives in `ofs-client`** (ofs-video is not a client dependency); its consistency with `ofs-video::link::FIELD_RATE_HZ` is carried debt.
10. **Latency quantization happens at register time** in `ofs-sim` (`VideoHandles` stores `world.receiver.latency_ms / 20 * 20`) and the quantized value is what the state message publishes.
11. **The DVR repeats the held frame:** a dropped field's slot writes the previous JPEG again (spec §5), so the recording freezes exactly where the pilot froze. Recording runs **only in the FPV view**; a viewport resize or aspect switch during recording stops it cleanly.
12. **Chase view stays live:** only the FPV picture is delayed (the chase view is the sim's spectator view, not goggles).
13. **MSAA moves onto the SubViewport** (`msaa_3d = 2`, 4×): project-level render settings do not apply to SubViewports.
14. **Wire additions:** `VideoLink` message fields `field_valid = 10` (bool), `latency_ms = 11` (uint32). GDExtension telemetry dict keys `video_field_valid`, `video_latency_ms`; Python `state.video.field_valid` / `state.video.latency_ms`. The FPV view dict exposes `video_sync` in the same string form as `get_telemetry` so `video.gd` is unchanged.
15. **`VideoSync` state getter:** `SyncTracker` (crates/ofs-video/src/link.rs:89) gains a `state()` accessor — the drop draw must know the sync state without mutating it.
16. **`ofs-video` gains `rand.workspace = true`** (the drop draw needs `rand::Rng`; the workspace already pins the version).

## Review Focus

The five inputs the spec implies but no task's tests fully exercise, most likely first — each is pinned by the named test:

1. **Consecutive dropped fields** (a long marginal-signal excursion): the client walks back at most to the oldest buffered valid field and never shows a future field — `test_many_drops_walk_back_bounded` (Task 3).
2. **`latency_ms = 0` or a missing `[receiver]` section:** the composite is still field-quantized, nothing freezes at the pad, no panic — `test_zero_latency_quantizes_nothing_away` (Task 2) + `test_zero_latency_picks_the_current_field` (Task 3).
3. **A DVR recording stopped with zero frames** (toggle on and off at once): a valid header-only AVI, an honest `frames = 0` summary, no panic — `test_zero_frames_writes_a_valid_empty_avi` (Task 4).
4. **Tramp garbage on the wire** (random bytes, truncated frames, echoes of our own responses): the parser resyncs, counts `bad_frames`, never panics — `proptest no byte sequence panics` (Task 6).
5. **A window resize or aspect switch during a recording:** the recording finalizes cleanly whenever `stop` arrives (headers patched, index written, summary honest) — pinned by `two_frames_round_trip_with_padding_and_a_patched_index` and `zero_frames_writes_a_valid_empty_avi` (Task 4); the resize path itself calls `dvr.stop()` before changing the viewport size (one guarded line in `_layout_fpv`, Tasks 5/9), exercised by the manual checks (Tasks 5 and 9).

## File Structure

```
crates/ofs-video/src/link.rs        + field-drop model, field_valid, SyncTracker::state()
crates/ofs-video/src/tramp.rs       new  Tramp codec (framing, CRC, requests, responses)
crates/ofs-video/src/vtx.rs         + codec dispatch (Smartaudio | Tramp), Tramp request handling
crates/ofs-video/Cargo.toml         + rand
crates/ofs-video/tests/link.rs      + drop tests
crates/ofs-video/tests/vtx.rs       + Tramp codec tests
crates/ofs-config/src/world.rs      + [receiver] latency_ms (+ validation)
crates/ofs-config/src/lib.rs        + VtxKind::Tramp
crates/ofs-core/src/names.rs        + VIDEO_FIELD_VALID
crates/ofs-sim/src/vehicle.rs       + VideoInfo{field_valid, latency_ms}, VideoHandles, vtx_model passes kind
crates/ofs-sim/src/session.rs       + video_msg maps the new fields
crates/ofs-proto/src/lib.rs         + PROTOCOL_VERSION 7
proto/ofs/v1/sim.proto              + VideoLink fields 10/11, header comment
python/ofs/v1/sim_pb2*              regenerated
crates/ofs-client/src/model.rs      + VideoInfo{field_valid, latency_ms}, from_pb
crates/ofs-client/src/interp.rs     + StateBuffer::{sim_now, pose_at_sim}
crates/ofs-client/src/fpv.rs        new  field buffer + delayed-composite selection (FpvView, FpvBuffer)
crates/ofs-client/src/dvr.rs        new  MJPEG AVI muxer on a worker thread
crates/ofs-client/src/worker.rs     + ingest feeds FpvBuffer; OSD history
crates/ofs-client/src/client.rs     + fpv_view(), dvr_start/write_frame/stop
crates/ofs-client/src/lib.rs        + module declarations
crates/ofs-client/tests/fpv.rs      new  composite selection tests
crates/ofs-client/tests/dvr.rs      new  AVI structure tests
crates/ofs-client/tests/model.rs    + field_valid/latency_ms
crates/ofs-godot/src/lib.rs         + get_fpv_view(), dvr_* funcs, telemetry keys
godot/main.tscn                     + FpvViewport/FpvBox/FpvTexture; 3D + layers move inside
godot/ui/dvr.gd                     new  capture timing + UX
godot/scripts/app.gd                + node paths, FPV delayed branch, DVR keybind/capture, aspect layout
godot/scripts/settings.gd           + fpv_aspect setting
godot/ui/controls_menu.gd           + aspect control
godot/ui/osd.gd                     + page-aware glyph drawing
godot/tests/test_osd.gd             + atlas/page tests
godot/tests/test_dvr.gd             new  file-side DVR tests
godot/tests/test_settings.gd        + fpv_aspect
godot/tests/test_controls_menu.gd   + aspect control
godot/tests/run_tests.gd            + test_dvr.gd
tools/make_osd_font.py              + optional second page
godot/ui/osd_font.png               regenerated (two pages)
godot/ui/OSD_FONT_LICENSE.md        + bold.mcm provenance
quads/opendrone-5f-tramp.toml       new  Tramp quad
quads/opendrone-5f-tramp.betaflight.diff  new
python/tests/test_sitl_tramp.py     new  live Tramp test
python/ofs/client.py                + VideoLink field_valid/latency_ms
python/tests/test_client.py         + field mapping
docs/dev-setup.md, README.md, docs/research/video-link.md, docs/superpowers/m3e-carried-debt.md
```

---

### Task 1: The FPV composite moves into a SubViewport

Pure restructure — no behaviour change. The 3D world, both cameras and the lens/OSD/video layers move inside a SubViewport shown through a full-window TextureRect; the HUD and controls menu stay at the root. Later tasks size the box and capture it.

**Files:**
- Modify: `godot/main.tscn`
- Modify: `godot/scripts/app.gd`
- Modify: `godot/ui/osd.gd` (`box_for` removed — the OSD fills the viewport)
- Modify: `godot/tests/test_osd.gd` (the box test replaced)

**Interfaces:**
- Produces: node paths `$FpvViewport` (SubViewport), `$FpvViewport/{World,Drone,FpvCamera,ChaseCamera,Lens,Osd,Video}`, `$FpvBox` (Control), `$FpvBox/FpvTexture` (TextureRect); `app.gd`'s `@onready` names `fpv_viewport`, `fpv_texture`, `fpv_box`; `func _layout_fpv()` (Task 1: full-window sizing only; Task 9 adds aspects); `osd.gd` draws the grid over the whole canvas (the composite's aspect is the OSD's aspect — the boxed picture is what changes it, Task 9).

- [ ] **Step 1: Rewrite the scene tree in `godot/main.tscn`**

The node blocks become (ext_resources and the `[gd_scene]` line unchanged):

```ini
[node name="Main" type="Node3D"]
script = ExtResource("1")

[node name="FpvViewport" type="SubViewport" parent="."]
handle_input_locally = false
size = Vector2i(1280, 720)
msaa_3d = 2

[node name="World" type="Node3D" parent="FpvViewport"]
script = ExtResource("2")

[node name="Drone" type="Node3D" parent="FpvViewport"]
script = ExtResource("3")

[node name="FpvCamera" type="Camera3D" parent="FpvViewport/Drone"]
current = true
script = ExtResource("4")

[node name="ChaseCamera" type="Camera3D" parent="FpvViewport"]
script = ExtResource("5")

[node name="Lens" type="CanvasLayer" parent="FpvViewport"]
layer = 1
script = ExtResource("6")

[node name="Osd" type="CanvasLayer" parent="FpvViewport"]
layer = 2
script = ExtResource("9")

[node name="Video" type="CanvasLayer" parent="FpvViewport"]
layer = 3
script = ExtResource("10")

[node name="FpvBox" type="Control" parent="."]
layout_mode = 3
anchors_preset = 15
anchor_right = 1.0
anchor_bottom = 1.0
grow_horizontal = 2
grow_vertical = 2
mouse_filter = 2

[node name="FpvTexture" type="TextureRect" parent="FpvBox"]
layout_mode = 0
offset_right = 1280.0
offset_bottom = 720.0
mouse_filter = 2

[node name="Hud" type="CanvasLayer" parent="."]
layer = 4
script = ExtResource("7")

[node name="ControlsMenu" type="CanvasLayer" parent="."]
layer = 5
script = ExtResource("8")
```

- [ ] **Step 2: Update `godot/scripts/app.gd` — node paths, texture wiring, layout**

Replace the `@onready` block (app.gd:20-28):

```gdscript
@onready var fpv_viewport: SubViewport = $FpvViewport
@onready var drone: Node3D = $FpvViewport/Drone
@onready var fpv_camera: Camera3D = $FpvViewport/Drone/FpvCamera
@onready var chase_camera: Camera3D = $FpvViewport/ChaseCamera
@onready var lens: CanvasLayer = $FpvViewport/Lens
@onready var osd: CanvasLayer = $FpvViewport/Osd
@onready var video: CanvasLayer = $FpvViewport/Video
@onready var world: Node3D = $FpvViewport/World
@onready var fpv_box: Control = $FpvBox
@onready var fpv_texture: TextureRect = $FpvBox/FpvTexture
@onready var hud: CanvasLayer = $Hud
@onready var controls_menu: CanvasLayer = $ControlsMenu
```

In `_ready()`, after the controls-menu setup lines, add:

```gdscript
	fpv_texture.texture = fpv_viewport.get_texture()
	get_viewport().size_changed.connect(_layout_fpv)
	_layout_fpv()
```

Add the layout function (Task 9 replaces the sizing rule with the aspect setting; today everything is full-window, which is also the chase-view rule forever):

```gdscript
## Sizes the FPV box and the viewport's internal resolution. The chase view and the "window" aspect fill the
## window; the boxed aspects (Task 9) center a fixed-resolution picture like goggles do.
func _layout_fpv() -> void:
	var window_size := fpv_box.size
	fpv_texture.position = Vector2.ZERO
	fpv_texture.size = window_size
	fpv_viewport.size = Vector2i(window_size)
```

- [ ] **Step 3: The OSD fills the viewport** (`godot/ui/osd.gd`)

Inside the SubViewport the composite's aspect IS the OSD's aspect — the OSD's own 4:3 box logic (`box_for`, osd.gd:60-67, and its use at osd.gd:101) is the M3a debt the spec supersedes. In `_draw_grid` (osd.gd:101-102) replace the box:

```gdscript
	var box := Rect2(Vector2.ZERO, _canvas.size)
```

and delete `box_for` together with `test_the_box_is_4_by_3_fitted_and_centred` in `godot/tests/test_osd.gd` (the drawn-cell and blink tests are unaffected — `_draw_grid` runs in the test tree where `_canvas.size` is the test control's size, as it was).

- [ ] **Step 4: Grep for stale node paths**

Run: `grep -rn '\$Drone\|\$World\|\$FpvCamera\|\$ChaseCamera\|\$Lens\|\$Osd\b\|\$Video\b' godot/ --include="*.gd" | grep -v FpvViewport`
Expected: only matches inside `app.gd`'s new `@onready` block (already updated). `test_scene.gd` and the e2e tests go through `app.world` / `app.drone` properties and need no changes.

- [ ] **Step 5: Verify the full Godot suite**

Run: `scripts/run-godot-tests.sh`
Expected: PASS (no new failures vs. the pre-task run).

- [ ] **Step 6: Verify the e2e flights**

Run: `godot --headless --path godot -s res://tests/e2e_open_loop.gd` (needs `cargo build -p ofs-sim -p ofs-godot` first)
Expected: PASS.

- [ ] **Step 7: Eyeball the pixels**

Run: `godot --path godot -s res://tests/shots.gd -- --out=<scratch dir>` and compare `start_fpv.png`, `start_chase.png`, `osd_fpv.png`, `video_grain.png` against a pre-task run (save one first if none exists).
Expected: identical-looking output through the new viewport — except the OSD now fills the picture box instead of sitting in its own 4:3 box on a wide window.

- [ ] **Step 8: Commit**

```bash
git add godot/main.tscn godot/scripts/app.gd
git commit -m "feat(godot): the FPV composite renders in a SubViewport (the DVR and aspect seam)"
```

---

### Task 2: Server field drops, latency config, protocol 7

`VideoLink` decides per field whether it arrived; the world file carries the glass-to-glass latency; the state message carries both. One wire change (protocol 7), all clients moved together.

**Files:**
- Modify: `crates/ofs-core/src/names.rs:124` (after `VIDEO_SYNC`)
- Modify: `crates/ofs-video/src/link.rs`
- Modify: `crates/ofs-video/Cargo.toml`
- Modify: `crates/ofs-video/tests/link.rs`
- Modify: `crates/ofs-config/src/world.rs:66-83` (`ReceiverSection`)
- Modify: `crates/ofs-config/src/world.rs:238-240` (`validate`)
- Modify: `crates/ofs-sim/src/vehicle.rs:119-181` (`VideoInfo`, `VideoHandles`)
- Modify: `crates/ofs-sim/src/session.rs:77-97` (`video_msg`)
- Modify: `proto/ofs/v1/sim.proto:1-6, 163-176`
- Modify: `crates/ofs-proto/src/lib.rs` (`PROTOCOL_VERSION`)
- Modify: `crates/ofs-proto/tests/messages.rs` (if it pins the literal version)
- Modify: `crates/ofs-client/src/model.rs:116-140` (`VideoInfo`)
- Modify: `crates/ofs-client/tests/model.rs`
- Modify: `crates/ofs-godot/src/lib.rs:186-250` (`get_telemetry` doc + keys)
- Modify: `python/ofs/client.py:101-113, 244-248` (`VideoLink`, `_video`)
- Modify: `python/tests/test_client.py`
- Regenerate: `python/ofs/v1/sim_pb2.py`, `sim_pb2.pyi`, `sim_pb2_grpc.py`

**Interfaces:**
- Produces: `ofs_video::link::{FIELD_DROP_P_MAX, field_drop_probability(snr_db: f64) -> f64}`, `SyncTracker::state(&self) -> Option<VideoSync>`, bus signal `video.field_valid` (1.0/0.0); `ofs_config::world::ReceiverSection.latency_ms: u32` (default 40, validated ≤ 1000); sim/client `VideoInfo` fields `field_valid: bool`, `latency_ms: u32`; pb `VideoLink.field_valid = 10`, `latency_ms = 11`; `PROTOCOL_VERSION = 7`; Python `VideoLink.field_valid: bool = False`, `latency_ms: int = 0`; Godot dict keys `video_field_valid`, `video_latency_ms`. Later tasks consume: `field_drop_probability` + the bus signal (Task 3), the telemetry keys (Task 3), the Python fields (Task 7).

- [ ] **Step 1: Write the failing drop-model tests** (`crates/ofs-video/tests/link.rs`, appended)

```rust
#[test]
fn the_field_drop_probability_peaks_between_unstable_and_lost() {
    close(field_drop_probability(25.0), 0.0, 1e-12, "a locked link never drops");
    close(field_drop_probability(6.0), 0.0, 1e-12, "at the unstable edge: none");
    close(field_drop_probability(4.5), FIELD_DROP_P_MAX * 0.5, 1e-12, "half way down the band");
    close(field_drop_probability(3.0), FIELD_DROP_P_MAX, 1e-12, "at the lost edge: the peak");
    close(field_drop_probability(2.0), 0.0, 1e-12, "a lost link is static, not held: no drops");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ofs-video --test link --locked`
Expected: FAIL — `field_drop_probability` and `FIELD_DROP_P_MAX` not found.

- [ ] **Step 3: Implement the drop model in `crates/ofs-video/src/link.rs`**

Add to `crates/ofs-video/Cargo.toml` under `[dependencies]`:

```toml
rand.workspace = true
```

Add the constant and function after `RELOCK_AFTER_FIELDS` (link.rs:38):

```rust
/// The chance the receiver fails to complete a field at the lost edge of the marginal band; the probability
/// ramps linearly from 0 at `UNSTABLE_SNR_DB` to this at `LOST_SNR_DB`, and is 0 outside the band (a locked
/// link never drops, a lost link outputs static — drops there would mean nothing). An estimate; see
/// docs/research/video-link.md.
pub const FIELD_DROP_P_MAX: f64 = 0.5;

/// The per-field drop probability at `snr_db` (M3e spec §2).
pub fn field_drop_probability(snr_db: f64) -> f64 {
    if snr_db >= UNSTABLE_SNR_DB || snr_db < LOST_SNR_DB {
        return 0.0;
    }
    FIELD_DROP_P_MAX * ramp(snr_db, UNSTABLE_SNR_DB, LOST_SNR_DB)
}
```

Add the accessor to `SyncTracker` (after `update`, link.rs:120):

```rust
    /// The current sync state without updating it (the drop decision reads it).
    pub fn state(&self) -> Option<VideoSync> {
        self.state
    }
```

Extend `Outputs` (link.rs:205) with `field_valid: Signal<f64>,` and register it in `VideoLink::new`'s `Outputs` initializer (link.rs:240):

```rust
            sync: bus.signal(names::VIDEO_SYNC),
            field_valid: bus.signal(names::VIDEO_FIELD_VALID),
```

Add to `crates/ofs-core/src/names.rs` after `VIDEO_SYNC`:

```rust
/// 1.0 when the receiver completed the field, 0.0 when it dropped it (M3e); 0.0 while nothing transmits.
pub const VIDEO_FIELD_VALID: &str = "video.field_valid";
```

Replace the tail of `VideoLink::step` (link.rs:329-344, from `let active = ...` through the last `bus.set(o.sync, ...)`) with:

```rust
        let active = if self.params.world.diversity { self.diversity.choose(&snr) } else { 0 };
        // At a marginal SNR the receiver may fail to complete the field: one seeded draw per field, after the
        // fading draws. A lost sync outputs static rather than held fields, so no drops are drawn there.
        let mut field_valid = transmitting;
        if transmitting && self.tracker.state() != Some(VideoSync::Lost) {
            if rand::Rng::gen(&mut self.rng) < field_drop_probability(snr[active]) {
                field_valid = false;
            }
        }
        let o = &self.out;
        if field_valid {
            let sync = self.tracker.update(snr[active]);
            let picture = picture(snr[active]);
            bus.set(o.present, 1.0);
            bus.set(o.snr, snr[active]);
            for (signal, value) in o.rssi.iter().zip(&rssi) {
                bus.set(*signal, *value);
            }
            bus.set(o.antenna, active as f64);
            bus.set(o.interference, interference[active]);
            bus.set(o.noise, picture.noise);
            bus.set(o.sparkles, picture.sparkles);
            bus.set(o.chroma, picture.chroma);
            bus.set(o.sync, sync as i32 as f64);
        } else {
            // No field arrived: the receiver repeats the previous one, so sync and the picture keep their last
            // values and only field_valid marks the miss.
            bus.set(o.present, 1.0);
        }
        bus.set(o.field_valid, if field_valid { 1.0 } else { 0.0 });
        Ok(())
```

Add `use rand::Rng;` to the imports at the top of link.rs.

- [ ] **Step 4: Run the ofs-video tests**

Run: `cargo test -p ofs-video --locked`
Expected: PASS (the new test, and all existing link tests — the fading draws are unchanged in order).

- [ ] **Step 5: Add the same-seed determinism test** (`crates/ofs-video/tests/link.rs`, appended — the file's header already imports `StepCtx`, `Model`, `names`, `Bus`, `DVec3` and `DQuat`)

```rust
#[test]
fn the_same_seed_drops_the_same_fields() {
    for seed in [1u64, 7, 42] {
        let drops = run_fields(seed);
        assert_eq!(drops, run_fields(seed), "seed {seed}: the same seed, the same drops");
        assert!(drops.iter().any(|v| !*v), "seed {seed}: the band actually drops some fields");
        assert!(drops.iter().any(|v| *v), "seed {seed}: the band does not drop everything");
    }
}

/// Steps a link in place for 200 fields with the quad parked 866 m out (25 mW: about 4.5 dB — the middle of
/// the drop band, since the bare link budget gives 8 dB at 580 m and 20 dB a decade) and reports each field's
/// `video.field_valid`.
fn run_fields(seed: u64) -> Vec<bool> {
    let params = bare(open_field());
    let mut bus = Bus::new();
    let pos = bus.signal::<DVec3>(names::BODY_POS_NED);
    bus.set(pos, DVec3::new(866.0, 0.0, -1.7));
    let att = bus.signal::<DQuat>(names::BODY_ATT);
    bus.set(att, DQuat::IDENTITY);
    let mut link = VideoLink::new(params, 1, seed, &mut bus);
    let vtx_present = bus.signal::<f64>(names::VTX_PRESENT);
    let vtx_freq = bus.signal::<f64>(names::VTX_FREQ_MHZ);
    let vtx_power = bus.signal::<f64>(names::VTX_POWER_MW);
    bus.set(vtx_present, 1.0);
    bus.set(vtx_freq, 5800.0);
    bus.set(vtx_power, 25.0);
    let field_valid = bus.signal::<f64>(names::VIDEO_FIELD_VALID);
    (0..200)
        .map(|i| {
            link.step(&StepCtx { tick: i, time_s: f64::from(i) * 0.02, dt_s: 0.02 }, &mut bus).unwrap();
            bus.get(field_valid) > 0.5
        })
        .collect()
}
```

The 4.5 dB placement is verified in the test run itself: if `drops.iter().all(|v| *v)` (the SNR landed above 6 dB) or everything dropped (below 3 dB — the static band draws nothing, so this means above, not below), move the 866.0 figure until some fields drop, recompute with the existing `snr_at(&bare(open_field()), 25.0, d)` helper, and note the chosen figure in the comment. (`StepCtx`'s exact field list is `{ tick, time_s, dt_s }` — as `tests/vtx.rs` constructs it.)

- [ ] **Step 6: Latency config in `crates/ofs-config/src/world.rs`**

Add to `ReceiverSection` (after `diversity`, world.rs:73):

```rust
    /// Glass-to-glass video latency in ms (camera + VTX + air + goggles), quantized down to whole fields by the
    /// simulator. Default: the 2-field estimate in docs/research/video-link.md.
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u32,
```

with, beside `default_noise_floor_dbm` (world.rs:77):

```rust
fn default_latency_ms() -> u32 {
    40
}
```

Add to `ReceiverSection`'s `Default`/constructor site (the `default_noise_floor_dbm: default_noise_floor_dbm(),` initializer, world.rs:212): `latency_ms: default_latency_ms(),`

Add to `validate` next to the `noise_floor_dbm` check (world.rs:239):

```rust
        c.check(r.latency_ms <= 1000, "receiver.latency_ms", format!("must be at most 1000 ms (got {})", r.latency_ms));
```

- [ ] **Step 7: Write the failing proto/state tests, then implement protocol 7**

Extend `proto/ofs/v1/sim.proto` — the header comment's first line becomes:

```
// Simulator control, protocol version 7: protocol 6 plus the video link's field validity and glass-to-glass latency (M3e).
```

and `VideoLink` gains:

```proto
  bool field_valid = 10;         // false: the receiver dropped this field (marginal signal)
  uint32 latency_ms = 11;        // the goggles' glass-to-glass latency, quantized down to whole fields
```

In `crates/ofs-proto/src/lib.rs`: `PROTOCOL_VERSION` 6 → 7. Run `grep -n "6" crates/ofs-proto/tests/messages.rs` — update any literal version pin to 7 (the import-based assertions need no change).

In `crates/ofs-sim/src/vehicle.rs`: add the quantization as a tested pure function near `VideoInfo`:

```rust
/// The glass-to-glass latency the pipeline can actually deliver: whole fields (20 ms at 50 Hz), rounded down.
pub fn quantize_latency_ms(ms: u32) -> u32 {
    ms / 20 * 20
}

#[test]
fn the_latency_quantizes_down_to_whole_fields() {
    assert_eq!(quantize_latency_ms(40), 40, "the default is already two fields");
    assert_eq!(quantize_latency_ms(37), 20, "37 ms delivers one field");
    assert_eq!(quantize_latency_ms(0), 0);
    assert_eq!(quantize_latency_ms(1015), 1000);
}
```

`VideoInfo` (vehicle.rs:119) gains

```rust
    /// Whether the receiver completed the last field.
    pub field_valid: bool,
    /// The world file's glass-to-glass latency, quantized down to whole fields.
    pub latency_ms: u32,
```

(`Default` gets `field_valid: false, latency_ms: 0`; the `sync: VideoSync::Locked` default stays.) `VideoHandles` gains `field_valid: Signal<f64>` and `latency_ms: u32`; `register` stores `latency_ms: quantize_latency_ms(world.receiver.latency_ms)` (the quantization ruling) and registers `field_valid: bus.signal(names::VIDEO_FIELD_VALID)`; `read` fills `field_valid: b.get(self.field_valid) > 0.5, latency_ms: self.latency_ms`. `vtx_model` is untouched here. In `crates/ofs-sim/src/session.rs`, `video_msg` (session.rs:77) gains `field_valid: v.field_valid, latency_ms: v.latency_ms,` in the `pb::VideoLink` literal, and the `the_video_message_says_absent_without_a_vtx` test (session.rs:562) additionally asserts `assert!(!msg.field_valid); assert_eq!(msg.latency_ms, 0);`.

- [ ] **Step 8: Regenerate the Python stubs and extend the Python/odot clients**

Run: `python -m grpc_tools.protoc -I proto --python_out=python --pyi_out=python --grpc_python_out=python proto/ofs/v1/sim.proto`

`crates/ofs-client/src/model.rs` `VideoInfo` (model.rs:116) gains `pub field_valid: bool, pub latency_ms: u32` (defaults `false`/`0`; `from_pb` maps `v.field_valid`, `v.latency_ms`). Update `crates/ofs-client/tests/model.rs` where `VideoInfo` literals are constructed. `crates/ofs-godot/src/lib.rs` `get_telemetry` adds after `video_chroma` (lib.rs:241):

```rust
        d.set("video_field_valid", v.field_valid);
        d.set("video_latency_ms", v.latency_ms);
```

and the doc comment above it lists both keys. `python/ofs/client.py` `VideoLink` (client.py:102) gains `field_valid: bool = False` and `latency_ms: int = 0` fields, and `_video` (client.py:244) maps `field_valid=m.field_valid, latency_ms=m.latency_ms`. Extend `python/tests/test_client.py`'s state-mapping test with `assert state.video.field_valid in (True, False)` style pins mirroring the existing video assertions (copy the neighboring pattern).

- [ ] **Step 9: Run everything**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && python -m pytest python/tests -v`
Expected: PASS (the live-SITL-marked tests skip without `OFS_SITL_LAUNCH`).

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat(proto)!: protocol 7 - the video link's field validity and glass-to-glass latency"
```

---

### Task 3: The client's delayed composite

One rule decides what the pilot sees: the newest field boundary at or before `sim_now − latency`, with pose, video quality and OSD all sampled there; a dropped field shows the previous one. The selection lives in `ofs-client` (unit-tested without Godot); `app.gd` switches its FPV branch to it.

**Files:**
- Create: `crates/ofs-client/src/fpv.rs`
- Modify: `crates/ofs-client/src/lib.rs` (module list)
- Modify: `crates/ofs-client/src/interp.rs:88-108` (`StateBuffer`)
- Modify: `crates/ofs-client/src/worker.rs:28-90` (`Shared`, `ingest`, `store_osd`)
- Modify: `crates/ofs-client/src/client.rs:25-70, 138-160` (`Client`, `Shared` construction, accessors)
- Create: `crates/ofs-client/tests/fpv.rs`
- Modify: `crates/ofs-godot/src/lib.rs` (new `get_fpv_view`)
- Modify: `godot/scripts/app.gd` (FPV branch)

**Interfaces:**
- Consumes: `VideoInfo.field_valid`/`latency_ms` (Task 2), telemetry dict keys `video_field_valid`/`video_latency_ms`.
- Produces: `fpv::{FIELD_PERIOD_S, field_floor(f64) -> f64, FieldUniforms, FpvView, FpvBuffer::{push_field, push_osd, clear, field_at, osd_at}}`; `StateBuffer::{sim_now(client_now_s: f64) -> Option<f64>, pose_at_sim(sim_time_s: f64) -> Option<Pose>}`; `Client::fpv_view(&self) -> Option<FpvView>`; GDExtension `get_fpv_view() -> Dictionary` with keys `pose` (Transform3D), `render_time_s`, `video_present`, `video_noise`, `video_sparkles`, `video_chroma`, `video_sync` (string, as `get_telemetry`), `field_valid`, `latency_ms`, `osd` (a `get_osd`-shaped Dictionary or empty); `app.gd` `_fpv_render_time` tracking. Task 5 consumes `get_fpv_view` and the render-instant discipline.- [ ] **Step 1: Write the failing selection tests** (`crates/ofs-client/tests/fpv.rs`, new)

```rust
use ofs_client::fpv::{field_floor, FieldUniforms, FpvBuffer};
use ofs_client::model::{OsdFrame, VideoSync};

fn uniforms(present: bool, valid: bool) -> (FieldUniforms, bool) {
    (
        FieldUniforms { present, noise: 0.1, sparkles: 0.0, chroma: 1.0, sync: VideoSync::Locked },
        valid,
    )
}

fn field(t: f64, valid: bool) -> (f64, FieldUniforms, bool) {
    let (u, v) = uniforms(true, valid);
    (field_floor(t), u, v)
}

fn push_fields(buffer: &mut FpvBuffer, times: &[f64], valid: &[bool]) {
    for (t, v) in times.iter().zip(valid) {
        let (u, _) = uniforms(true, *v);
        buffer.push_field(*t, u, *v);
    }
}

#[test]
fn the_field_boundaries_quantize_to_twenty_milliseconds() {
    assert_eq!(field_floor(0.019), 0.0);
    assert_eq!(field_floor(0.02), 0.02);
    assert_eq!(field_floor(1.037), 1.02, "37 ms of latency quantizes down to one field");
}

#[test]
fn samples_within_one_field_replace_it() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.004, 0.008], &[true, true, true]);
    let mut marker = uniforms(true, true).0;
    marker.noise = 0.9;
    b.push_field(0.0199, marker, true); // a newer sample of the same field
    let (t, u, valid) = b.field_at(0.02).unwrap();
    assert_eq!(t, 0.0);
    assert_eq!(u.noise, 0.9, "the newest sample of a field replaces its entry");
    assert!(valid);
}

#[test]
fn a_dropped_field_holds_the_previous_one() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.02, 0.04], &[true, false, true]);
    let (t, _, _) = b.field_at(0.06).unwrap();
    assert_eq!(t, 0.02, "field 0.04 dropped: the newest valid one before it shows");
}

#[test]
fn many_drops_walk_back_bounded() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0], &[true]);
    push_fields(&mut b, &[0.02, 0.04, 0.06], &[false, false, false]);
    let (t, _, _) = b.field_at(0.08).unwrap();
    assert_eq!(t, 0.0, "the walk stops at the oldest buffered valid field");
    assert!(b.field_at(field_floor(-0.01)).is_none(), "nothing before the first field");
}

#[test]
fn zero_latency_picks_the_current_field() {
    let mut b = FpvBuffer::default();
    push_fields(&mut b, &[0.0, 0.02, 0.04], &[true, true, true]);
    // latency 0: the target is field_floor(sim_now) — the newest received field, nothing held back.
    let (t, _, _) = b.field_at(field_floor(0.059)).unwrap();
    assert_eq!(t, 0.04);
}

#[test]
fn the_osd_picks_the_newest_frame_at_or_before_the_instant() {
    let mut b = FpvBuffer::default();
    b.push_osd(OsdFrame { seq: 1, time_s: 0.01, present: true, cols: 1, rows: 1, cells: vec![0x41] });
    b.push_osd(OsdFrame { seq: 2, time_s: 0.031, present: true, cols: 1, rows: 1, cells: vec![0x42] });
    assert_eq!(b.osd_at(0.03).unwrap().seq, 1, "a frame drawn after the instant is not shown yet");
    assert_eq!(b.osd_at(0.05).unwrap().seq, 2, "exactly at the boundary the newer frame counts as drawn");
    assert!(b.osd_at(0.0).is_none());
}
```

If `OsdFrame`'s fields or `VideoSync`'s variant names differ (check `crates/ofs-client/src/model.rs:274-290` and the `VideoSync` enum), match them exactly — the test compiles against the real types or not at all.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ofs-client --test fpv --locked`
Expected: FAIL — the `fpv` module does not exist.

- [ ] **Step 3: Implement `crates/ofs-client/src/fpv.rs`**

```rust
//! The FPV picture as the goggles compose it (M3e spec §3): pose, video quality and OSD all sampled at one
//! delayed, field-quantized instant, so the picture ages as a unit at the field rate and a dropped field holds
//! the previous one. The render instant is the newest field boundary at or before `sim_now − latency`.
use std::collections::VecDeque;

use crate::interp::Pose;
use crate::model::{OsdFrame, VideoSync};

/// One analog video field: 20 ms of simulated time (50 fields a second; the same rate as
/// `ofs-video::link::FIELD_RATE_HZ`, duplicated here because ofs-video is not a client dependency —
/// carried debt records the pairing).
pub const FIELD_PERIOD_S: f64 = 0.02;
/// Fields kept (1.28 s of picture) and OSD frames kept (0.64 s of OSD): ample for a goggle's latency.
const MAX_FIELDS: usize = 64;
const MAX_OSD_FRAMES: usize = 32;

/// What a received field carries for the shader (the state's picture values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldUniforms {
    pub present: bool,
    pub noise: f64,
    pub sparkles: f64,
    pub chroma: f64,
    pub sync: VideoSync,
}

/// What to draw this frame: the pose, the picture and the OSD of one received field.
#[derive(Debug, Clone, PartialEq)]
pub struct FpvView {
    pub pose: Pose,
    pub uniforms: FieldUniforms,
    /// Whether the shown field arrived (false: it is a held older field).
    pub field_valid: bool,
    pub osd: Option<OsdFrame>,
    /// Simulated time of the field the picture shows (a field boundary).
    pub render_time_s: f64,
}

/// The newest field boundary at or before `sim_time_s`.
pub fn field_floor(sim_time_s: f64) -> f64 {
    (sim_time_s / FIELD_PERIOD_S).floor() * FIELD_PERIOD_S
}

#[derive(Debug, Default)]
pub struct FpvBuffer {
    /// One entry per received field: (field boundary, uniforms, the field arrived).
    fields: VecDeque<(f64, FieldUniforms, bool)>,
    osd: VecDeque<OsdFrame>,
}

impl FpvBuffer {
    /// Adds a state sample's field. The state stream is faster than the video, so samples inside one field
    /// replace that field's entry; a sample from before the newest one clears the buffer (the session restarted).
    pub fn push_field(&mut self, time_s: f64, uniforms: FieldUniforms, valid: bool) {
        if !(time_s.is_finite() && uniforms.noise.is_finite() && uniforms.chroma.is_finite()) {
            return;
        }
        let boundary = field_floor(time_s);
        if let Some(&(last, _, _)) = self.fields.back() {
            if boundary == last {
                let entry = self.fields.back_mut().unwrap();
                entry.1 = uniforms;
                entry.2 = valid;
                return;
            }
            if boundary < last {
                self.clear();
            }
        }
        self.fields.push_back((boundary, uniforms, valid));
        if self.fields.len() > MAX_FIELDS {
            self.fields.pop_front();
        }
    }

    pub fn push_osd(&mut self, frame: OsdFrame) {
        if !frame.time_s.is_finite() {
            return;
        }
        self.osd.push_back(frame);
        if self.osd.len() > MAX_OSD_FRAMES {
            self.osd.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.fields.clear();
        self.osd.clear();
    }

    /// The field whose picture to show for the render instant `target_s` (a field boundary): the newest field at
    /// or before it that arrived — a dropped field holds the previous one. The bool is whether that field
    /// arrived. None when every buffered field before the instant was dropped; the caller keeps the last picture.
    pub fn field_at(&self, target_s: f64) -> Option<(f64, FieldUniforms, bool)> {
        let mut i = self.fields.iter().rposition(|(t, _, _)| *t <= target_s + 1e-9)?;
        while !self.fields[i].2 {
            i = i.checked_sub(1)?;
        }
        Some(self.fields[i])
    }

    /// The newest OSD frame drawn no later than `target_s` (an exactly-at-the-instant frame counts as drawn).
    pub fn osd_at(&self, target_s: f64) -> Option<OsdFrame> {
        self.osd.iter().rev().find(|f| f.time_s <= target_s + 1e-9).cloned()
    }
}
```

Add `pub mod fpv;` to `crates/ofs-client/src/lib.rs` beside the other modules.

- [ ] **Step 4: Extend `StateBuffer`** (`crates/ofs-client/src/interp.rs`, after `pose_at`)

```rust
    /// The simulator's idea of "now" for `client_now_s`: the least-delayed recent arrival's clock offset
    /// (delays only add, so the minimum is the best estimate). None before the first sample and while paused
    /// (the offsets are forgotten then) — the caller keeps the last picture.
    pub fn sim_now(&self, client_now_s: f64) -> Option<f64> {
        let latest = *self.samples.back()?;
        let offset = self.offsets.iter().copied().reduce(f64::min)?;
        Some((client_now_s - offset).max(latest.sim_time_s))
    }

    /// The pose at an exact simulated time (no clock offset, no display delay): interpolated between the two
    /// samples that bracket it; the oldest or newest pose held outside them.
    pub fn pose_at_sim(&self, sim_time_s: f64) -> Option<Pose> {
        if self.samples.is_empty() {
            return None;
        }
        if sim_time_s <= self.samples.front()?.sim_time_s {
            return Some((*self.samples.front()?).into());
        }
        let latest = *self.samples.back()?;
        if sim_time_s >= latest.sim_time_s {
            return Some(latest.into());
        }
        let after = self.samples.partition_point(|s| s.sim_time_s <= sim_time_s);
        let (a, b) = (self.samples[after - 1], self.samples[after]);
        let t = (sim_time_s - a.sim_time_s) / (b.sim_time_s - a.sim_time_s);
        Some(Pose { pos: a.pos.lerp(b.pos, t), att: a.att.slerp(b.att, t) })
    }
```

- [ ] **Step 5: Wire the buffers through the worker and client**

`crates/ofs-client/src/worker.rs` `Shared` (worker.rs:30-40) gains `pub fpv: Mutex<FpvBuffer>,`; the `Shared::new`-equivalent constructor (worker.rs:58-66) initializes `fpv: Mutex::new(FpvBuffer::default())`. `store_osd` (worker.rs:71) becomes:

```rust
    fn store_osd(&self, frame: Option<OsdFrame>) {
        *lock(&self.osd) = frame.clone();
        let mut fpv = lock(&self.fpv);
        match frame {
            Some(f) => fpv.push_osd(f),
            None => fpv.clear(),
        }
        drop(fpv);
        self.osd_version.fetch_add(1, Ordering::AcqRel);
    }
```

(Adapt `lock` and the clone to the file's actual helper names — `store_osd` currently assigns through the mutex guard directly.) `ingest` (worker.rs:76-89) builds the telemetry once and feeds the field buffer:

```rust
    fn ingest(&self, state: pb::State) {
        let arrival_s = self.epoch.elapsed().as_secs_f64();
        let telemetry = Telemetry::from_pb(&state);
        if let (Some(p), Some(q)) = (state.position_ned_m, state.attitude) {
            let sample = Sample {
                sim_time_s: state.time_s,
                pos: vec_to_godot(DVec3::new(p.x, p.y, p.z)),
                att: quat_to_godot(DQuat::from_xyzw(q.x, q.y, q.z, q.w)),
            };
            lock(&self.buffer).push(sample, arrival_s, state.running);
        }
        let v = &telemetry.video;
        lock(&self.fpv).push_field(
            state.time_s,
            FieldUniforms { present: v.present, noise: v.noise, sparkles: v.sparkles, chroma: v.chroma, sync: v.sync },
            v.field_valid,
        );
        *lock(&self.telemetry) = Some((telemetry, Instant::now()));
    }
```

`crates/ofs-client/src/client.rs`: the `Client` gains (beside `osd`/`osd_version`):

```rust
    /// The FPV picture for this frame (the goggles' pipeline): the newest field boundary at or before
    /// `sim_now − latency` with the pose, video uniforms and OSD all sampled there; a dropped field holds the
    /// previous one. None before the first state, while paused, or when every buffered field was dropped —
    /// the caller keeps the last picture.
    pub fn fpv_view(&self) -> Option<FpvView> {
        let now_s = self.shared.epoch.elapsed().as_secs_f64();
        let buffer = lock(&self.shared.buffer);
        let sim_now = buffer.sim_now(now_s)?;
        let latency_ms = lock(&self.shared.telemetry).clone().map_or(0, |(t, _)| t.video.latency_ms);
        let target = fpv::field_floor(sim_now - f64::from(latency_ms) / 1000.0);
        let (render_time_s, uniforms, field_valid) = lock(&self.shared.fpv).field_at(target)?;
        let pose = buffer.pose_at_sim(render_time_s)?;
        let osd = lock(&self.shared.fpv).osd_at(render_time_s);
        Some(FpvView { pose, uniforms, field_valid, osd, render_time_s, latency_ms })
    }
```

(with `use crate::fpv::{self, FpvView, FieldUniforms};` added).

- [ ] **Step 6: Run the Rust tests**

Run: `cargo test -p ofs-client --locked && cargo clippy -p ofs-client --all-targets --locked -- -D warnings`
Expected: PASS.

- [ ] **Step 7: Expose `get_fpv_view` in the GDExtension** (`crates/ofs-godot/src/lib.rs`)

First factor the OSD-dictionary building out of `get_osd` (lib.rs:255-268) into a helper both functions use:

```rust
fn osd_dict(frame: &OsdFrame) -> VarDictionary {
    let cells: Vec<i32> = frame.cells.iter().map(|c| *c as i32).collect();
    let mut d = VarDictionary::new();
    d.set("seq", frame.seq as i64);
    d.set("time_s", frame.time_s);
    d.set("present", frame.present);
    d.set("cols", i64::from(frame.cols));
    d.set("rows", i64::from(frame.rows));
    d.set("cells", &PackedInt32Array::from(cells.as_slice()));
    d
}
```

(`get_osd` becomes `let Some(frame) = ... else { return VarDictionary::new() }; osd_dict(&frame)`.) Then the new accessor, after `get_telemetry`:

```rust
    /// The FPV picture for this frame — the goggles' pipeline: the pose, video uniforms and OSD of one delayed,
    /// field-quantized instant; a dropped field holds the previous one. Empty before the first field: `pose`
    /// (Transform3D), `render_time_s`, `video_present`, `video_noise`, `video_sparkles`, `video_chroma`,
    /// `video_sync` ("locked"/"unstable"/"lost"), `field_valid`, `latency_ms`, and `osd` (the Dictionary
    /// `get_osd` returns, or empty).
    #[func]
    fn get_fpv_view(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(v) = self.client.as_ref().and_then(|c| c.fpv_view()) else { return d };
        let rotation = Quaternion::new(v.pose.att.x as f32, v.pose.att.y as f32, v.pose.att.z as f32, v.pose.att.w as f32);
        let origin = Vector3::new(v.pose.pos.x as f32, v.pose.pos.y as f32, v.pose.pos.z as f32);
        d.set("pose", Transform3D::new(Basis::from_quaternion(rotation), origin));
        d.set("render_time_s", v.render_time_s);
        d.set("video_present", v.uniforms.present);
        d.set("video_noise", v.uniforms.noise);
        d.set("video_sparkles", v.uniforms.sparkles);
        d.set("video_chroma", v.uniforms.chroma);
        d.set("video_sync", &GString::from(v.uniforms.sync.as_str()));
        d.set("field_valid", v.field_valid);
        d.set("latency_ms", v.latency_ms);
        match &v.osd {
            Some(frame) => d.set("osd", &osd_dict(frame)),
            None => d.set("osd", &VarDictionary::new()),
        }
        d
    }
```

`FpvView` gains `pub latency_ms: u32` (set in `Client::fpv_view` from the same telemetry read the latency computation uses — hoist `let latency_ms = ...` above the `target` line and pass it into the view).

- [ ] **Step 8: Switch `app.gd`'s FPV branch to the delayed composite**

In `_process` (app.gd:77-104), replace the pose + video + osd section with:

```gdscript
	if not chase_view:
		var fpv: Dictionary = client.get_fpv_view()
		if fpv.has("pose"):
			drone.transform = fpv["pose"]
			video.update_view({
				"video_present": fpv["video_present"], "video_noise": fpv["video_noise"],
				"video_sparkles": fpv["video_sparkles"], "video_chroma": fpv["video_chroma"],
				"video_sync": fpv["video_sync"],
			}, delta)
		var render_time: float = fpv.get("render_time_s", -1.0)
		if render_time != _fpv_render_time:
			_fpv_render_time = render_time
			osd.set_frame(fpv.get("osd", {}))
	elif client.has_pose():
		drone.transform = client.get_pose()
```

Declare `var _fpv_render_time := -1.0` beside `_osd_version` (app.gd:40), reset it in `_on_phase_changed`'s `loading` branch (`_fpv_render_time = -1.0`), and delete the now-unused `_osd_version` mechanism only if nothing else reads `get_osd_version` (the chase view does not draw the OSD; keep the client API). The HUD's `update_view` keeps the live telemetry — the goggles' overlay is not the picture.

- [ ] **Step 9: The e2e asserts the delayed instant** (`godot/tests/e2e_open_loop.gd`, in the flight section, following the file's `_check` pattern)

```gdscript
	var fpv: Dictionary = app.client.get_fpv_view()
	_check(fpv.has("pose"), "the fpv view carries a pose")
	_check(fpv.has("render_time_s") and float(fpv["render_time_s"]) > 0.0, "the render instant is set")
	_check(bool(fpv.get("field_valid", false)), "a clean link completes every field")
	_check(int(fpv.get("latency_ms", -1)) == 40, "the default latency reaches the client")
	_check(float(fpv["render_time_s"]) <= float(app.client.get_telemetry()["time_s"]), "the picture trails the sim clock")
```

(The last check is the latency in its weakest honest form — with the 40 ms default and the 20 ms quantization, the render instant is at least one field behind the newest state sample.)

- [ ] **Step 10: Verify**

Run: `scripts/run-godot-tests.sh && godot --headless --path godot -s res://tests/e2e_open_loop.gd && cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS. Manual check (only if a window is available): fly, and the FPV picture visibly trails the chase view by about 2 fields at the default latency.

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "feat(client): the FPV picture ages as a unit - pose, quality and OSD at one delayed field-quantized instant"
```

---

### Task 4: The DVR muxer (`ofs-client::dvr`)

A standard MJPEG AVI written frame by frame on a worker thread, sizes patched at stop. Pure Rust, golden-tested by parsing its own output.

**Files:**
- Create: `crates/ofs-client/src/dvr.rs`
- Modify: `crates/ofs-client/src/lib.rs` (module list)
- Create: `crates/ofs-client/tests/dvr.rs`

**Interfaces:**
- Produces: `dvr::{DvrSummary { frames, dropped_frames, bytes }, DvrWriter::{start(path: PathBuf, fps: u32, width: u32, height: u32) -> Result<DvrWriter, String>, write_frame(&self, jpeg: Vec<u8>), stop(self) -> Result<DvrSummary, String>}}`, const `DVR_FPS: u32 = 50`. Task 5 consumes `DvrWriter` through the `Client`.

- [ ] **Step 1: Write the failing structure tests** (`crates/ofs-client/tests/dvr.rs`, new)

```rust
//! The DVR container's structure, checked by parsing what it wrote: a minimal RIFF/AVI walker.
use ofs_client::dvr::DvrWriter;
use std::path::PathBuf;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn tag(&mut self) -> [u8; 4] {
        let t = self.bytes[self.pos..self.pos + 4].try_into().unwrap();
        self.pos += 4;
        t
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.bytes[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn u16(&mut self) -> u16 {
        let v = u16::from_le_bytes(self.bytes[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        v
    }
    fn skip(&mut self, n: usize) {
        self.pos += n;
    }
    fn expect(&mut self, t: &[u8; 4], what: &str) {
        assert_eq!(self.pos + 4 <= self.bytes.len(), true, "truncated at {what}");
        assert_eq!(self.tag(), t, "{what}");
    }
}

/// Walks RIFF down to the `movi` payloads and returns (frame sizes, index entries, total length).
fn parse(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<(u32, u32, u32)>, usize) {
    let mut r = Reader { bytes, pos: 0 };
    r.expect(b"RIFF", "riff");
    let riff_size = r.u32();
    r.expect(b"AVI ", "avi");
    r.expect(b"LIST", "hdrl list");
    r.u32();
    r.expect(b"hdrl", "hdrl");
    r.expect(b"avih", "avih");
    assert_eq!(r.u32(), 56, "avih size");
    let _us_per_frame = r.u32();
    r.skip(8); // max bytes, padding
    let flags = r.u32();
    assert_eq!(flags & 0x10, 0x10, "AVIF_HASINDEX");
    r.skip(4); // total frames (patched)
    r.skip(4); // initial frames
    assert_eq!(r.u32(), 1, "one stream");
    r.skip(4 + 8 + 4 + 16); // suggested buffer, width, height, reserved
    r.expect(b"LIST", "strl list");
    r.u32();
    r.expect(b"strl", "strl");
    r.expect(b"strh", "strh");
    assert_eq!(r.u32(), 56, "strh size");
    r.expect(b"vids", "stream type");
    r.expect(b"MJPG", "handler");
    r.skip(4 + 2 + 2 + 4); // flags, priority, language, initial frames
    let scale = r.u32();
    let rate = r.u32();
    assert_eq!((scale, rate), (1, 50), "50 fps");
    r.skip(4); // start
    let length = r.u32();
    r.skip(4 + 4 + 4 + 8); // suggested, quality, sample size, rc frame
    r.expect(b"strf", "strf");
    assert_eq!(r.u32(), 40, "bitmapinfoheader size");
    let width = i32::from_le_bytes(bytes[r.pos..r.pos + 4].try_into().unwrap());
    let height = i32::from_le_bytes(bytes[r.pos + 4..r.pos + 8].try_into().unwrap());
    assert_eq!((width, height), (4, 2));
    r.skip(8);
    assert_eq!(&bytes[r.pos..r.pos + 4], b"MJPG", "biCompression");
    r.pos = bytes.windows(4).position(|w| w == b"movi").unwrap() + 4;
    let movi_data = r.pos;
    let mut payloads = Vec::new();
    loop {
        if &bytes[r.pos..r.pos + 4] == b"idx1" {
            break;
        }
        let tag = r.tag();
        assert_eq!(&tag, b"00dc", "every frame is a 00dc chunk, got {tag:?}");
        let size = r.u32() as usize;
        payloads.push(bytes[r.pos..r.pos + size].to_vec());
        r.skip(size + (size & 1)); // the pad byte on odd sizes
    }
    r.expect(b"idx1", "index");
    let index_size = r.u32() as usize;
    let mut index = Vec::new();
    for i in 0..index_size / 16 {
        let _tag = r.tag();
        let flags = r.u32();
        let offset = r.u32();
        let size = r.u32();
        assert_eq!(flags, 0x10, "frame {i}: AVIIF_KEYFRAME");
        assert_eq!(offset, 4 + payloads[..i].iter().map(|p| p.len() + 8 + (p.len() & 1)).sum::<usize>() as u32, "frame {i} offset");
        assert_eq!(size as usize, payloads[i].len(), "frame {i} size");
        index.push((flags, offset, size));
    }
    assert_eq!(length as usize, payloads.len(), "strh length matches");
    assert_eq!(riff_size as usize + 8, bytes.len(), "the outer RIFF size is patched");
    (payloads, index, bytes.len())
}

fn write_two_frames(path: &PathBuf) -> ofs_client::dvr::DvrSummary {
    let writer = DvrWriter::start(path.clone(), 50, 4, 2).unwrap();
    writer.write_frame(b"AAA".to_vec());
    writer.write_frame(b"BBBBB".to_vec());
    writer.stop().unwrap()
}

#[test]
fn two_frames_round_trip_with_padding_and_a_patched_index() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.avi");
    let summary = write_two_frames(&path);
    assert_eq!(summary.frames, 2);
    assert_eq!(summary.dropped_frames, 0);
    let bytes = std::fs::read(&path).unwrap();
    let (payloads, index, len) = parse(&bytes);
    assert_eq!(payloads, vec![b"AAA".to_vec(), b"BBBBB".to_vec()]);
    assert_eq!(index.len(), 2);
    assert_eq!(summary.bytes as usize, len);
    assert!(bytes.windows(4).filter(|w| *w == b"00dc").count() >= 4, "movi chunks + index entries");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn zero_frames_writes_a_valid_empty_avi() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.avi");
    let writer = DvrWriter::start(path.clone(), 50, 4, 2).unwrap();
    let summary = writer.stop().unwrap();
    assert_eq!(summary.frames, 0);
    let bytes = std::fs::read(&path).unwrap();
    let (payloads, _, _) = parse(&bytes);
    assert!(payloads.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_directory_is_created_and_a_bad_frame_size_does_not_corrupt() {
    let dir = std::env::temp_dir().join(format!("ofs-dvr-mkdir-{}", std::process::id()));
    let path = dir.join("nested/deep/rec.avi");
    let writer = DvrWriter::start(path.clone(), 50, 4, 2).unwrap();
    writer.write_frame(vec![0xFF; 40]); // whatever bytes: the muxer does not validate JPEG content
    let summary = writer.stop().unwrap();
    assert_eq!(summary.frames, 1);
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ofs-client --test dvr --locked`
Expected: FAIL — `ofs_client::dvr` does not exist.

- [ ] **Step 3: Implement `crates/ofs-client/src/dvr.rs`**

```rust
//! The goggles' DVR container (M3e spec §5): an MJPEG AVI written frame by frame. Standard RIFF/AVI — a `LIST
//! hdrl` (avih + strh/strf declaring one 'vids'/'MJPG' stream at the fixed frame rate), a `LIST movi` with one
//! `00dc` JPEG chunk per frame (padded to even sizes), an `idx1` index, and the four size/length fields patched
//! when the recording stops. Encoding is Godot's (`Image.save_jpg_to_buffer`); this module only muxes. File I/O
//! runs on a worker thread so a slow disk cannot stall the render loop; at most [`FRAME_CHANNEL_CAPACITY`]
//! frames are in flight and further ones are dropped and counted (a recording that hiccups is fine, a flight
//! that stutters is not).
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::JoinHandle;

/// Frames in flight to the writer thread before the capture starts dropping: a second's worth.
const FRAME_CHANNEL_CAPACITY: usize = 50;
/// The capture cadence: recordings are tied to received fields.
pub const DVR_FPS: u32 = 50;
const AVIIF_KEYFRAME: u32 = 0x10;

/// What a finished recording holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DvrSummary {
    pub frames: u64,
    pub dropped_frames: u64,
    pub bytes: u64,
}

struct Header {
    microsec_per_frame: u32,
    fps: u32,
    width: u32,
    height: u32,
}

fn put(bytes: &mut Vec<u8>, tag: &[u8; 4]) {
    bytes.extend_from_slice(tag);
}

fn put32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Writes the fixed layout. Returns the file offsets of the four fields patched at stop: the outer RIFF size,
/// the `movi` LIST size, `avih.dwTotalFrames` and `strh.dwLength`. (The index's offsets are relative to the
/// position right after the `movi` fourcc, counting the fourcc: the first frame sits at 4.)
fn write_header(file: &mut File, h: &Header) -> std::io::Result<[usize; 4]> {
    let mut b = Vec::with_capacity(224);
    put(&mut b, b"RIFF");
    let riff_size_at = 4;
    put32(&mut b, 0);
    put(&mut b, b"AVI ");
    put(&mut b, b"LIST");
    put32(&mut b, 192); // hdrl: "hdrl" + avih + strl
    put(&mut b, b"hdrl");
    put(&mut b, b"avih");
    put32(&mut b, 56);
    put32(&mut b, h.microsec_per_frame);
    put32(&mut b, 0); // max bytes per second
    put32(&mut b, 0); // padding granularity
    put32(&mut b, 0x10); // AVIF_HASINDEX
    let avih_frames_at = 48;
    put32(&mut b, 0); // dwTotalFrames, patched
    put32(&mut b, 0); // initial frames
    put32(&mut b, 1); // streams
    put32(&mut b, 0); // suggested buffer
    put32(&mut b, h.width);
    put32(&mut b, h.height);
    for _ in 0..4 {
        put32(&mut b, 0); // reserved
    }
    put(&mut b, b"LIST");
    put32(&mut b, 116); // strl: "strl" + strh + strf
    put(&mut b, b"strl");
    put(&mut b, b"strh");
    put32(&mut b, 56);
    put(&mut b, b"vids");
    put(&mut b, b"MJPG");
    put32(&mut b, 0); // flags
    put16(&mut b, 0); // priority
    put16(&mut b, 0); // language
    put32(&mut b, 0); // initial frames
    put32(&mut b, 1); // scale
    put32(&mut b, h.fps); // rate
    put32(&mut b, 0); // start
    let strh_length_at = 140;
    put32(&mut b, 0); // dwLength, patched
    put32(&mut b, 0); // suggested buffer
    put32(&mut b, 10_000); // quality
    put32(&mut b, 0); // sample size
    put16(&mut b, 0); // rcFrame
    put16(&mut b, 0);
    put16(&mut b, h.width as u16);
    put16(&mut b, h.height as u16);
    put(&mut b, b"strf");
    put32(&mut b, 40); // BITMAPINFOHEADER
    put32(&mut b, h.width as i32 as u32);
    put32(&mut b, h.height as i32 as u32);
    put16(&mut b, 1); // planes
    put16(&mut b, 24); // bits
    put(&mut b, b"MJPG"); // biCompression
    put32(&mut b, h.width * h.height * 3); // biSizeImage
    for _ in 0..4 {
        put32(&mut b, 0); // X/Y pels, colors used, colors important
    }
    debug_assert_eq!(b.len(), 212);
    put(&mut b, b"LIST");
    let movi_size_at = 216;
    put32(&mut b, 0); // patched
    put(&mut b, b"movi");
    file.write_all(&b)?;
    Ok([riff_size_at, movi_size_at, avih_frames_at, strh_length_at])
}

fn writer(mut file: File, header: Header, rx: Receiver<Vec<u8>>) -> DvrSummary {
    let [riff_size_at, movi_size_at, avih_frames_at, strh_length_at] =
        write_header(&mut file, &header).expect("the DVR header write failed");
    let mut frames = 0u64;
    let mut index: Vec<u32> = Vec::new();
    while let Ok(jpeg) = rx.recv() {
        let size = jpeg.len() as u32;
        file.write_all(b"00dc").and_then(|_| file.write_all(&size.to_le_bytes())).and_then(|_| file.write_all(&jpeg)).expect("the DVR frame write failed");
        if jpeg.len() % 2 == 1 {
            file.write_all(&[0u8]).expect("the DVR pad write failed");
        }
        index.push(size);
        frames += 1;
    }
    // The sender is gone: the queue is drained, finalize.
    let mut idx = Vec::with_capacity(8 + index.len() * 16);
    put(&mut idx, b"idx1");
    put32(&mut idx, (index.len() * 16) as u32);
    let mut offset = 4u32;
    for size in &index {
        put(&mut idx, b"00dc");
        put32(&mut idx, AVIIF_KEYFRAME);
        put32(&mut idx, offset);
        put32(&mut idx, *size);
        offset += 8 + size + (size & 1);
    }
    file.write_all(&idx).expect("the DVR index write failed");
    let end = file.stream_position().expect("the DVR position failed") as u64;
    let movi_end = end - idx.len() as u64;
    let _ = file.seek(SeekFrom::Start(avih_frames_at as u64));
    let _ = file.write_all(&frames.to_le_bytes());
    let _ = file.seek(SeekFrom::Start(strh_length_at as u64));
    let _ = file.write_all(&frames.to_le_bytes());
    let _ = file.seek(SeekFrom::Start(movi_size_at as u64));
    // The movi LIST's size counts the 'movi' fourcc plus every chunk (movi_data_at = movi_size_at + 8).
    let _ = file.write_all(&(movi_end - (movi_size_at + 4)).to_le_bytes());
    let _ = file.seek(SeekFrom::Start(riff_size_at as u64));
    let _ = file.write_all(&((end - 8).to_le_bytes()));
    let _ = file.flush();
    DvrSummary { frames, dropped_frames: 0, bytes: end }
}
```

```rust
/// A recording in progress. `stop` finalizes; dropping without `stop` also finalizes (the writer thread joins).
pub struct DvrWriter {
    tx: Option<SyncSender<Vec<u8>>>,
    handle: Option<JoinHandle<DvrSummary>>,
    dropped: std::sync::Arc<AtomicU64>,
    path: PathBuf,
}

impl DvrWriter {
    /// Starts a recording at `fps` frames a second with `width` × `height` pixels; every `write_frame` must
    /// hand JPEG bytes of that size (Godot's encoder does). Creates the file's directory.
    pub fn start(path: PathBuf, fps: u32, width: u32, height: u32) -> Result<DvrWriter, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let header = Header { microsec_per_frame: 1_000_000 / fps.max(1), fps, width, height };
        let (tx, rx) = sync_channel(FRAME_CHANNEL_CAPACITY);
        let dropped = std::sync::Arc::new(AtomicU64::new(0));
        let dropped_in_thread = dropped.clone();
        let handle = std::thread::Builder::new()
            .name("ofs-dvr".into())
            .spawn(move || {
                let mut summary = writer(file, header, rx);
                summary.dropped_frames += dropped_in_thread.load(Ordering::Relaxed);
                summary
            })
            .map_err(|e| e.to_string())?;
        Ok(DvrWriter { tx: Some(tx), handle: Some(handle), dropped, path })
    }

    /// The recording's file, for toasts.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Hands one frame to the writer. JPEG bytes of the declared size. When the writer thread is saturated the
    /// frame is dropped and counted.
    pub fn write_frame(&self, jpeg: Vec<u8>) {
        let Some(tx) = &self.tx else { return };
        if tx.try_send(jpeg).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Stops: drops the sender (the worker drains the queue and patches the headers) and joins.
    pub fn stop(mut self) -> Result<DvrSummary, String> {
        self.tx.take();
        let handle = self.handle.take().ok_or_else(|| "the recorder already stopped".to_string())?;
        handle.join().map_err(|_| "the DVR writer panicked".to_string())
    }
}

impl Drop for DvrWriter {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
```

Add `pub mod dvr;` to `crates/ofs-client/src/lib.rs`.

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo test -p ofs-client --test dvr --locked && cargo clippy -p ofs-client --all-targets --locked -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Verify by eye outside the test**

Write one recording to a scratch dir via the test binary (`cargo test -p ofs-client --test dvr -- --nocapture` after temporarily pointing `two.avi` at a fixed path) and open it in VLC/mpv — or trust the parser test plus Task 5's manual check. If VLC is installed, do it now; otherwise note it for Task 5's manual check.

- [ ] **Step 6: Commit**

```bash
git add crates/ofs-client/src/dvr.rs crates/ofs-client/src/lib.rs crates/ofs-client/tests/dvr.rs
git commit -m "feat(client): the DVR muxer - a self-contained MJPEG AVI on a worker thread"
```

---

### Task 5: DVR capture and UX

Godot captures the SubViewport once per received field (repeating the held frame on drops), the Rust `Client` owns the `DvrWriter`, a keybind and toasts drive it, and it records in the FPV view only.

**Files:**
- Modify: `crates/ofs-client/src/client.rs` (`dvr_start`, `dvr_write_frame`, `dvr_stop`)
- Modify: `crates/ofs-godot/src/lib.rs` (the three `#[func]` wrappers)
- Create: `godot/ui/dvr.gd`
- Modify: `godot/main.tscn` (a `Dvr` node)
- Modify: `godot/scripts/app.gd` (keybind, capture hook, resize stop)
- Create: `godot/tests/test_dvr.gd`
- Modify: `godot/tests/run_tests.gd` (register the file)

**Interfaces:**
- Consumes: `DvrWriter` (Task 4), `get_fpv_view()` and `_fpv_render_time` (Task 3).
- Produces: `Client::{dvr_start(&self, path: String, fps: u32, width: u32, height: u32) -> Result<(), String>, dvr_write_frame(&self, jpeg: Vec<u8>), dvr_stop(&self) -> Result<DvrSummary, String>}`; GDExtension `dvr_start(path: GString, width: i64, height: i64) -> GString` ("" or the error; fps is `DVR_FPS` inside), `dvr_write_frame(jpeg: PackedByteArray)`, `dvr_stop() -> Dictionary` (`frames`, `dropped_frames`, `bytes`, `path`); `godot/ui/dvr.gd` `start(client, viewport) -> String`, `capture(view)`, `stop() -> Dictionary`, `recording: bool`, signals `started(path)`, `stopped(summary)`; app.gd `KEY_D` toggles.

- [ ] **Step 1: Client-side recording ownership** (`crates/ofs-client/src/client.rs`)

The `Client` gains `dvr: Mutex<Option<DvrWriter>>` (initialized `Mutex::new(None)`) and:

```rust
    /// Starts a DVR recording to `path` at the field rate with `width` × `height` pixels.
    pub fn dvr_start(&self, path: String, width: u32, height: u32) -> Result<(), String> {
        let mut dvr = lock(&self.dvr);
        if dvr.is_some() {
            return Err("already recording".into());
        }
        *dvr = Some(DvrWriter::start(PathBuf::from(path), dvr::DVR_FPS, width, height)?);
        Ok(())
    }

    pub fn dvr_write_frame(&self, jpeg: Vec<u8>) {
        let dvr = lock(&self.dvr);
        if let Some(w) = dvr.as_ref() {
            w.write_frame(jpeg);
        }
    }

    /// Stops the recording; the summary reports frames, drops and bytes.
    pub fn dvr_stop(&self) -> Result<DvrSummary, String> {
        let writer = lock(&self.dvr).take().ok_or_else(|| "not recording".to_string())?;
        writer.stop()
    }
```

- [ ] **Step 2: The GDExtension wrappers** (`crates/ofs-godot/src/lib.rs`, after `get_fpv_view`)

```rust
    /// Starts a DVR recording to `path` (an absolute .avi path) at the viewport's pixel size; "" or why not.
    #[func]
    fn dvr_start(&self, path: GString, width: i64, height: i64) -> GString {
        let Some(client) = self.client.as_ref() else { return GString::from("no session") };
        match client.dvr_start(path.to_string(), width.max(1) as u32, height.max(1) as u32) {
            Ok(()) => GString::new(),
            Err(e) => GString::from(e),
        }
    }

    #[func]
    fn dvr_write_frame(&self, jpeg: PackedByteArray) {
        if let Some(client) = self.client.as_ref() {
            client.dvr_write_frame(jpeg.to_vec());
        }
    }

    /// Stops the recording: `{frames, dropped_frames, bytes, path}`, or empty when it was not recording.
    #[func]
    fn dvr_stop(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let Some(client) = self.client.as_ref() else { return d };
        let Ok(summary) = client.dvr_stop() else { return d };
        d.set("frames", summary.frames as i64);
        d.set("dropped_frames", summary.dropped_frames as i64);
        d.set("bytes", summary.bytes as i64);
        d
    }
```

(If `dvr_stop` wants the path in the toast, have `Client::dvr_stop` include `path` in the returned summary via `DvrSummary` gaining `path: PathBuf` — then `d.set("path", ...)`; choose one and use it in both layers. The plan pins: `DvrSummary` stays as Task 4 defined it, and `dvr.gd` already knows the path it passed.)

- [ ] **Step 3: `godot/ui/dvr.gd`** (new)

```gdscript
extends Node
## The goggles' DVR (M3e spec §5): records the FPV composite — the SubViewport's picture, degradation and OSD
## included, exactly what the pilot sees — to an MJPEG AVI. The Rust side muxes (OfsClient.dvr_*); this node
## times the captures to the received fields and owns the UX. Records in the FPV view only; a resize or aspect
## switch while recording stops it (app.gd).

signal started(path: String)
signal stopped(summary: Dictionary)

const JPEG_QUALITY := 80

var recording := false
var _client: Object
var _viewport: SubViewport
var _last_render_time := -1.0
var _last_jpeg := PackedByteArray()


func start(client: Object, viewport: SubViewport) -> String:
	var path := "user://recordings/REC_%s.avi" % _timestamp()
	var error: String = client.dvr_start(ProjectSettings.globalize_path(path), viewport.size.x, viewport.size.y)
	if error != "":
		return error
	recording = true
	_client = client
	_viewport = viewport
	_last_render_time = -1.0
	_last_jpeg = PackedByteArray()
	started.emit(path)
	return ""


## Feeds one field. `view` is the Dictionary `OfsClient.get_fpv_view()` returns: each new field boundary
## captures the picture; a dropped one repeats the held frame, so the recording freezes where the pilot froze.
## A frame the size of the viewport's texture; when the size changes app.gd stops the recording.
func capture(view: Dictionary) -> void:
	if not recording or view.is_empty():
		return
	var t: float = view.get("render_time_s", -1.0)
	if t == _last_render_time:
		return
	_last_render_time = t
	if not bool(view.get("field_valid", false)):
		if not _last_jpeg.is_empty():
			_client.dvr_write_frame(_last_jpeg)
		return
	var image: Image = _viewport.get_texture().get_image()
	if image == null or image.is_empty():
		return  # headless or the texture not yet rendered: this field is simply not in the recording
	_last_jpeg = image.save_jpg_to_buffer(JPEG_QUALITY)
	_client.dvr_write_frame(_last_jpeg)


func stop() -> Dictionary:
	if not recording:
		return {}
	recording = false
	var summary: Dictionary = _client.dvr_stop()
	stopped.emit(summary)
	return summary


func _timestamp() -> String:
	return Time.get_datetime_string_from_system(true).replace(":", "").replace("-", "").replace("T", "_")
```

Add to `godot/main.tscn` (after ControlsMenu):

```ini
[node name="Dvr" type="Node" parent="."]
script = ExtResource("11")
```

with `[ext_resource type="Script" path="res://ui/dvr.gd" id="11"]` added to the ext_resources.

- [ ] **Step 4: Wire the keybind, capture and resize into `app.gd`**

Add `@onready var dvr: Node = $Dvr` beside the other `@onready` vars. In `_process`, at the end of the FPV branch from Task 3 (after the OSD update): `dvr.capture(fpv)`. In `_unhandled_key_input`'s match (app.gd:109-132):

```gdscript
		KEY_D:
			_toggle_dvr()
```

with:

```gdscript
func _toggle_dvr() -> void:
	if chase_view:
		hud.add_toast("DVR records the FPV view - switch with C first", "warn")
		return
	if dvr.recording:
		var summary: Dictionary = dvr.stop()
		if summary.is_empty():
			hud.add_toast("DVR failed: the writer stopped abnormally", "error")
		else:
			hud.add_toast("DVR: %d frames saved%s" % [summary.get("frames", 0), _dvr_drop_note(summary)])
	else:
		var error: String = dvr.start(client, fpv_viewport)
		hud.add_toast("DVR: recording to user://recordings" if error == "" else "DVR failed: %s" % error, "info" if error == "" else "error")
		if error != "":
			return


func _dvr_drop_note(summary: Dictionary) -> String:
	var dropped: int = summary.get("dropped_frames", 0)
	return "" if dropped == 0 else " (%d dropped)" % dropped
```

A resize or aspect switch during a recording stops it cleanly (Review Focus 5): at the end of `_layout_fpv` (Task 1):

```gdscript
	if dvr != null and dvr.recording and fpv_viewport.size != Vector2i(window_size):
		var summary: Dictionary = dvr.stop()
		hud.add_toast("DVR stopped: the picture size changed (%d frames saved)" % summary.get("frames", 0), "warn")
```

placed **before** the `fpv_viewport.size = ...` assignment so the comparison sees the old size. In `_ready`, the `dvr.stopped` signal is not needed by app.gd (the callers toast directly); leave it for tests.

- [ ] **Step 5: The file-side tests** (`godot/tests/test_dvr.gd`, new) — headless has no renderer, so the tests drive the client's DVR API with synthetic frame bytes and parse the file; the live capture path is Task 5's manual check plus `shots.gd`.

```gdscript
extends "res://tests/testing.gd"
## The DVR's file side: the client's dvr_* API writes a parseable MJPEG AVI (headless has no renderer, so the
## capture path itself is checked by eye and by shots.gd; test_dvr.gd drives the bytes).

func _client() -> Object:
	ok(ClassDB.class_exists("OfsClient"), "the extension is loaded")
	var client: Object = ClassDB.instantiate("OfsClient")
	add_child(client)
	return client


func _count_frames(path: String) -> int:
	var f := FileAccess.open(path, FileAccess.READ)
	ok(f != null, "the recording exists")
	var bytes := f.get_buffer(f.get_length())
	var count := 0
	for i in range(bytes.size() - 3):
		if bytes.subarray(i, i + 3) == PackedByteArray([0x30, 0x30, 0x64, 0x63]):
			count += 1
	return count  # movi chunks + idx1 entries: an even count, frames = count / 2


func test_a_recording_round_trips() -> void:
	var client := _client()
	var path := OS.get_user_data_dir() + "/test_dvr_roundtrip.avi"
	eq(client.dvr_start(path, 4, 2), "", "starts")
	for i in 10:
		client.dvr_write_frame(PackedByteArray([0xFF, 0xD8, 0xFF, 0xD9, i]))  # a JPEG-shaped blob; the muxer does not validate
	var summary: Dictionary = client.dvr_stop()
	eq(int(summary.get("frames", -1)), 10, "ten frames")
	eq(int(summary.get("dropped_frames", -1)), 0, "nothing dropped")
	eq(_count_frames(path) / 2, 10, "ten 00dc chunks in movi and ten index entries")
	var f := FileAccess.open(path, FileAccess.READ)
	eq(f.get_buffer(4).get_string_from_ascii(), "RIFF", "it is a RIFF file")
	client.free()


func test_stop_without_start_and_double_start_are_honest() -> void:
	var client := _client()
	eq(client.dvr_stop().size(), 0, "stop without start is empty")
	var path := OS.get_user_data_dir() + "/test_dvr_double.avi"
	eq(client.dvr_start(path, 4, 2), "", "first start")
	ok(client.dvr_start(path, 4, 2) != "", "a second start fails")
	client.dvr_stop()
	client.free()
```

Register in `godot/tests/run_tests.gd` after `test_video.gd`: `"res://tests/test_dvr.gd",`. Clean the user-data test files up in the test's end (or leave them; the user data dir is disposable).

- [ ] **Step 6: Verify**

Run: `cargo build -p ofs-godot && scripts/run-godot-tests.sh`
Expected: PASS. Then `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` — PASS.

- [ ] **Step 7: Manual check (a window is needed)**

Fly open-loop, press D, fly a few seconds, press D again; play `user://recordings/REC_*.avi` ( `%APPDATA%/Godot/app_userdata/<project>/recordings/`) in VLC: it plays at 50 fps, shows the degraded picture with the OSD, and freezes during link loss. Toggle D in the chase view: the warn toast appears.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(godot): the goggles' DVR - the FPV composite recorded to a self-contained AVI"
```

---

### Task 6: The Tramp codec

A second VTX protocol behind the existing `VtxModel`, selected by `[vtx] kind = "tramp"`. The codec owns the wire (16-byte frames, sum-of-bytes-1..13 CRC, requests `r`/`v`/`F`/`P`/`I`, responses `r`/`v`/`s`); the model keeps its state.

**Files:**
- Create: `crates/ofs-video/src/tramp.rs`
- Modify: `crates/ofs-video/src/vtx.rs` (codec dispatch + Tramp handling)
- Modify: `crates/ofs-video/src/lib.rs` (module list)
- Modify: `crates/ofs-config/src/lib.rs:299-304` (`VtxKind`)
- Modify: `crates/ofs-video/tests/vtx.rs`

**Interfaces:**
- Consumes: `VtxState`/`VtxParams`/`VtxModel` (unchanged shapes); `VtxKind` (config).
- Produces: `tramp::{Request, Parser::{push(&mut self, &[u8]) -> Vec<Request>, bad_frames(&self) -> u32}, response_frame(code: u8, payload: &[u8]) -> Vec<u8>, crc(bytes: &[u8]) -> u8, TRAMP_FRAME_LENGTH = 16}` with `Request::{Reset, GetSettings, SetFrequency(u16), SetPowerMw(u16), SetActive(bool)}`; `VtxModel::new(kind: VtxKind, params, requests, replies, divisor, bus)` (the kind moves to the first parameter); `VtxModel::bad_frames()` counts both codecs. Task 7 consumes `VtxKind::Tramp` + the model.

- [ ] **Step 1: Write the failing codec tests** (`crates/ofs-video/tests/vtx.rs`, appended — copy the file's existing `use` lines' style)

```rust
use ofs_video::tramp::{self, Parser, Request as TrampRequest};
use ofs_video::VtxKind;

fn tramp_frame(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x0F, cmd];
    f.extend_from_slice(payload);
    f.resize(14, 0);
    f.push(tramp::crc(&f[1..14]));
    f.push(0x00);
    f
}

#[test]
fn the_tramp_crc_is_the_sum_of_bytes_one_to_thirteen() {
    // Pinned against the pinned Betaflight's vtx_tramp.c trampChecksum: 'F' 5800 MHz.
    let f = tramp_frame(b'F', &5800u16.to_le_bytes());
    assert_eq!(f[14], (0x46 + 0xA8 + 0x16) as u8, "0x0F itself is excluded");
    assert_eq!(f[15], 0x00);
}

#[test]
fn tramp_requests_decode_across_chunk_boundaries() {
    let mut p = Parser::default();
    let mut frames = Vec::new();
    frames.extend(tramp_frame(b'F', &5800u16.to_le_bytes()));
    frames.extend(tramp_frame(b'P', &200u16.to_le_bytes()));
    frames.extend(tramp_frame(b'I', &1u16.to_le_bytes()));
    frames.extend(tramp_frame(b'v', &[]));
    let mut got = Vec::new();
    for chunk in frames.chunks(5) {
        got.extend(p.push(chunk));
    }
    assert_eq!(
        got,
        vec![
            TrampRequest::SetFrequency(5800),
            TrampRequest::SetPowerMw(200),
            TrampRequest::SetActive(true),
            TrampRequest::GetSettings,
        ]
    );
    assert_eq!(p.bad_frames(), 0);
}

#[test]
fn tramp_garbage_resyncs_and_counts() {
    let mut p = Parser::default();
    p.push(&[0x01, 0x02, 0x0F, 0x46, 0x00, 0x00, 0xFF, 0xFF]); // a bad crc inside a frame's shape
    assert_eq!(p.bad_frames(), 1, "the bad frame counted");
    assert_eq!(p.push(&tramp_frame(b'v', &[])), vec![TrampRequest::GetSettings], "the parser resynced");
}

#[test]
fn tramp_responses_are_sixteen_bytes_with_the_same_crc() {
    // 'v' for R1 5658 MHz, 200 mW, not in pit: pinned by hand (ruling 1).
    let resp = tramp::response_frame(b'v', &[
        0x1A, 0x16, // freq 5658
        0xC8, 0x00, // configured power 200
        0x00,       // control mode
        0x00,       // pit off
        0xC8, 0x00, // actual power 200
        0, 0, 0, 0, // rest of the 12-byte payload
    ]);
    assert_eq!(resp.len(), 16);
    assert_eq!(resp[1], b'v');
    assert_eq!(resp[14], 0x36, "the checksum the FC recomputes");
    assert_eq!(resp[15], 0x00);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p ofs-video --test vtx --locked`
Expected: FAIL — `ofs_video::tramp` does not exist.

- [ ] **Step 3: Implement `crates/ofs-video/src/tramp.rs`**

```rust
//! The Tramp protocol from the VTX's side (M3e spec §6), checked against `vtx_tramp.c` of the pinned
//! Betaflight (2026.6.2; see the plan's ruling 1): 16-byte frames `0x0F cmd <12-byte payload> crc 0x00`, the
//! CRC a u8 sum of bytes 1..=13 (the 0x0F excluded — `trampChecksum`). The FC asks with 'r' (limits), 'v'
//! (settings), 'F'/'P'/'I' (frequency / power in mW / pit state) and only accepts 'r', 'v' and 's' back; 'F',
//! 'P' and 'I' are confirmed by the next 'v' at least 200 ms later.

/// The payload's u16 fields are little endian (the FC composes `param & 0xff`, `(param >> 8) & 0xff`).
pub const TRAMP_FRAME_LENGTH: usize = 16;
pub const CMD_RESET: u8 = b'r';
pub const CMD_GET_SETTINGS: u8 = b'v';
pub const CMD_SET_FREQ: u8 = b'F';
pub const CMD_SET_POWER: u8 = b'P';
pub const CMD_SET_ACTIVE: u8 = b'I';
pub const RESP_STATUS: u8 = b's';

/// The CRC Betaflight's Tramp driver recomputes: bytes 1..=13, the sync byte excluded.
pub fn crc(frame: &[u8]) -> u8 {
    frame[1..14].iter().fold(0u8, |sum, b| sum.wrapping_add(*b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Reset,
    GetSettings,
    SetFrequency(u16),
    SetPowerMw(u16),
    /// true: transmit (leave pit mode); false: enter pit mode.
    SetActive(bool),
}

/// A response frame as the FC's receive framer accepts it: any 16-byte frame with a valid CRC; the FC itself
/// reads only 'r', 'v' and 's' codes.
pub fn response_frame(code: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![0x0F, code];
    f.extend_from_slice(payload);
    f.resize(14, 0);
    f.push(crc(&f));
    f.push(0x00);
    f
}

#[derive(Debug, Default)]
pub struct Parser {
    buf: [u8; TRAMP_FRAME_LENGTH],
    pos: usize,
    bad_frames: u32,
}

impl Parser {
    /// Feeds bytes; returns the complete, valid requests in order. A frame with a bad CRC or an unknown
    /// command is ignored (as a real VTX would ignore it) and counted in [`bad_frames`](Self::bad_frames).
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Request> {
        let mut out = Vec::new();
        for &b in bytes {
            // Resync: hunt the 0x0F sync byte at a frame boundary, then fill 15 more.
            if self.pos == 0 && b != 0x0F {
                continue;
            }
            self.buf[self.pos] = b;
            self.pos += 1;
            if self.pos < TRAMP_FRAME_LENGTH {
                continue;
            }
            self.pos = 0;
            let frame = &self.buf;
            if crc(frame) == frame[14] && frame[15] == 0x00 {
                match Request::decode(frame) {
                    Some(request) => out.push(request),
                    None => self.bad_frames += 1,
                }
            } else {
                self.bad_frames += 1;
            }
        }
        out
    }

    pub fn bad_frames(&self) -> u32 {
        self.bad_frames
    }
}

impl Request {
    fn decode(frame: &[u8; TRAMP_FRAME_LENGTH]) -> Option<Request> {
        let param = u16::from_le_bytes([frame[2], frame[3]]);
        let rest_zero = frame[4..14].iter().all(|b| *b == 0);
        match frame[1] {
            CMD_RESET if rest_zero => Some(Request::Reset),
            CMD_GET_SETTINGS if rest_zero => Some(Request::GetSettings),
            CMD_SET_FREQ if rest_zero => Some(Request::SetFrequency(param)),
            CMD_SET_POWER if rest_zero => Some(Request::SetPowerMw(param)),
            CMD_SET_ACTIVE if rest_zero => Some(Request::SetActive(param != 0)),
            _ => None,
        }
    }
}
```

Add `pub mod tramp;` to `crates/ofs-video/src/lib.rs`.

- [ ] **Step 4: Run them to verify they pass**

Run: `cargo test -p ofs-video --test vtx --locked`
Expected: PASS.

- [ ] **Step 5: Wire the codec choice through the model**

`crates/ofs-config/src/lib.rs` `VtxKind` (lib.rs:299) gains:

```rust
pub enum VtxKind {
    /// SmartAudio v2.1 (fidelity level 1).
    Smartaudio,
    /// Tramp (fidelity level 1).
    Tramp,
}
```

`crates/ofs-video/src/vtx.rs`: `VtxModel` gains `kind: VtxKind`, and the SmartAudio parser becomes one arm of

```rust
enum Codec {
    Smartaudio(smartaudio::RequestParser),
    Tramp(tramp::Parser),
}
```

(`parser: RequestParser` → `codec: Codec`; `bad_frames()` delegates: `match &self.codec { Codec::Smartaudio(p) => p.bad_frames(), Codec::Tramp(p) => p.bad_frames() }`; the SmartAudio arm's requests flow through the existing `handle`.) `VtxModel::new`'s first parameter becomes `kind: VtxKind`, choosing the codec. Tramp requests, handled before `publish` like the SmartAudio ones:

```rust
    fn handle_tramp(&mut self, request: tramp::Request) -> Option<Vec<u8>> {
        match request {
            // Limits: the model accepts the SmartAudio frequency window (5000..5999) and its own power table.
            tramp::Request::Reset => {
                let payload = limits_payload(&self.params.power_levels_mw);
                Some(tramp::response_frame(tramp::CMD_RESET, &payload))
            }
            tramp::Request::GetSettings => {
                let freq = self.state.freq_mhz();
                let power_mw = self.params.power_levels_mw[self.state.power_level];
                let payload = settings_payload(freq, power_mw, self.state.pit);
                Some(tramp::response_frame(tramp::CMD_GET_SETTINGS, &payload))
            }
            tramp::Request::SetFrequency(freq) => {
                if (5000..=5999).contains(&freq) {
                    self.state.user_freq_mhz = Some(freq);
                }
                None // confirmed by the FC's next 'v' (ruling 2)
            }
            tramp::Request::SetPowerMw(mw) => {
                if let Some(level) = self.params.power_levels_mw.iter().position(|p| *p == u32::from(mw)) {
                    self.state.power_level = level;
                }
                None
            }
            tramp::Request::SetActive(active) => {
                self.state.pit = !active;
                None
            }
        }
    }
```

with the two payload builders beside it (both zero-padded to 12 bytes by `response_frame`'s resize):

```rust
/// 'r': min frequency 5000, max 5999 (the model's accepted window), max power from the model's table —
/// little endian; the FC ignores the reply unless min_freq is not zero.
fn limits_payload(power_levels_mw: &[u32]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&5000u16.to_le_bytes());
    p.extend_from_slice(&5999u16.to_le_bytes());
    p.extend_from_slice(&(*power_levels_mw.iter().max().unwrap_or(&0)).min(u32::from(u16::MAX)) as u16);
    p
}

/// 'v': frequency, configured power, control mode 0, pit flag, actual power — little endian; the FC ignores
/// the reply unless the frequency is not zero, which it never is while the VTX exists.
fn settings_payload(freq_mhz: u16, power_mw: u32, pit: bool) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&freq_mhz.to_le_bytes());
    p.extend_from_slice(&(power_mw.min(u32::from(u16::MAX)) as u16).to_le_bytes());
    p.push(0); // control mode: no race lock
    p.push(u8::from(pit));
    p.extend_from_slice(&(power_mw.min(u32::from(u16::MAX)) as u16).to_le_bytes());
    p
}
```

The temperature query arrives as a request: the FC's `trampQuery('s')` sends a `'s'`-commanded frame (`trampSendU16('s', 0)`), which the parser must decode. Add `Request::GetStatus` to the enum (`pub const CMD_GET_STATUS: u8 = b's';`, decoded like the others with `rest_zero`), and the model answers with `Some(tramp::response_frame(tramp::RESP_STATUS, &25u16.to_le_bytes()))` — 25 °C, the same constant the KISS ESC frames carry (M3a's thermal debt applies; the FC accepts any non-zero temperature). In `step`, the Tramp arm dispatches each request through `handle_tramp` and writes the `Some` replies to `self.replies` like the SmartAudio arm does.

`crates/ofs-sim/src/vehicle.rs` `vtx_model` (vehicle.rs:384) passes the kind: `Ok(VtxModel::new(x.kind, params, requests, replies, divisor, bus))`. (`x.kind` is `ofs_config::VtxKind`, re-exported or imported as needed.)

- [ ] **Step 6: Model-level tests** (`crates/ofs-video/tests/vtx.rs`, appended — using the file's existing `Rig` pattern: a `Wire`-wired `VtxModel` on a `Bus`, stepped with `StepCtx { tick: 0, time_s, dt_s: 0.001 }`)

```rust
use ofs_video::vtx::VtxKind;

/// The file's `params()` with the Tramp table (the FC's built-in power list, ruling 3) and one step up.
fn tramp_params() -> VtxParams {
    VtxParams {
        power_levels_mw: vec![25, 100, 200, 400, 600],
        power_levels_dbm: vec![14, 20, 23, 26, 28],
        default_band: band_index("R").unwrap(),
        default_channel: 1,
        default_power_index: 2,
        reply_latency_s: 0.005,
    }
}

fn tramp_rig() -> Rig {
    let mut bus = Bus::new();
    let requests = Wire::new(1024);
    let replies = Wire::new(1024);
    let model = VtxModel::new(VtxKind::Tramp, tramp_params(), requests.clone(), replies.clone(), 1, &mut bus);
    Rig { model, bus, requests, replies }
}

#[test]
fn the_tramp_vtx_answers_r_v_s_and_applies_f_p_i() {
    let mut rig = tramp_rig();

    // Init handshake: 'r' then 'v'.
    let reply = rig.ask(0.0, &tramp_frame(b'r', &[]));
    assert_eq!(reply.len(), 16, "the 'r' limits reply");
    assert_eq!(reply[1], b'r');
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5000, "the FC discards a zero min_freq");

    let reply = rig.ask(0.5, &tramp_frame(b'v', &[]));
    assert_eq!(reply[1], b'v');
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5658, "R1, the power-up default");
    assert_eq!(u16::from_le_bytes([reply[4], reply[5]]), 200, "default_power_index 2 of the tramp table");
    assert_eq!(reply[7], 0, "not in pit");

    // Frequency and power apply silently; pit mode flips with 'I' (0 = enter pit).
    rig.requests.write(&tramp_frame(b'F', &5800u16.to_le_bytes()));
    rig.requests.write(&tramp_frame(b'P', &600u16.to_le_bytes()));
    rig.requests.write(&tramp_frame(b'I', &0u16.to_le_bytes()));
    rig.step(1.0);
    rig.step(1.006);
    assert_eq!(rig.replies.take(1024).len(), 0, "F/P/I get no reply (ruling 2)");
    assert_eq!(rig.signal("vtx.freq_mhz"), 5800.0);
    assert_eq!(rig.signal("vtx.power_mw"), 600.0);
    assert_eq!(rig.signal("vtx.pit"), 1.0);

    // The FC's confirmation query reflects everything, including the pit flag.
    let reply = rig.ask(1.5, &tramp_frame(b'v', &[]));
    assert_eq!(u16::from_le_bytes([reply[2], reply[3]]), 5800);
    assert_eq!(reply[7], 1, "pit on");

    let reply = rig.ask(2.0, &tramp_frame(b's', &[]));
    assert_eq!(reply[1], b's');
    assert_eq!(i16::from_le_bytes([reply[6], reply[7]]), 25, "the constant temperature");
}
```

(The `Rig` struct and its `ask`/`step`/`signal` helpers exist in `tests/vtx.rs`; if `Rig`'s constructor hard-codes the SmartAudio `params()`, generalize it to take the `VtxParams` and `VtxKind` — the file's existing tests pass `VtxKind::Smartaudio` at the new second parameter. `vtx.freq_mhz`/`vtx.power_mw`/`vtx.pit` are the bus names `VtxModel::publish` sets — `names::VTX_FREQ_MHZ` etc.)

Then the fuzz test (the M1 rule: a parser that never panics, whatever arrives):

```rust
proptest! {
    #[test]
    fn tramp_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        let mut p = Parser::default();
        let _ = p.push(&bytes);
    }
}
```

with `use proptest::prelude::*;` added and `proptest.workspace = true` appended to `[dev-dependencies]` in `crates/ofs-video/Cargo.toml` (the same pattern as `crates/ofs-radio` and `crates/ofs-client`).

- [ ] **Step 7: Run the whole crate**

Run: `cargo test -p ofs-video --locked && cargo clippy -p ofs-video --all-targets --locked -- -D warnings`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(video): the Tramp codec behind the VtxModel - [vtx] kind = \"tramp\" works"
```

---

### Task 7: The Tramp quad and the live test

A second quad config exercises the codec against real Betaflight; the live test pins the round trip.

**Files:**
- Create: `quads/opendrone-5f-tramp.toml`
- Create: `quads/opendrone-5f-tramp.betaflight.diff`
- Create: `python/tests/test_sitl_tramp.py`

**Interfaces:**
- Consumes: `VtxKind::Tramp` + the codec (Task 6); the Python `load(quad_path=...)` kwarg and `state.vtx` / `state.video`.
- Produces: the live-test pattern the CI sitl job picks up automatically (`python -m pytest python/tests` runs the whole directory).

- [ ] **Step 1: The quad file** — copy `quads/opendrone-5f-freestyle.toml` and change only the `[vtx]` section and the header comment:

```toml
# OpenDrone 5" freestyle with a Tramp VTX — the M3e live-test quad. Everything but the VTX matches the shipped
# quad (M1 estimates). Units are in the key suffixes (SI). All values are estimates.
schema_version = 4
name = "OpenDrone 5F freestyle (Tramp VTX)"
```

```toml
# A Tramp VTX. Power levels are the FC's built-in Tramp table (ruling 3); the dBm column is a label — Tramp
# talks mW and the codec never sends it. Betaflight converts band/channel config to nothing (Tramp is
# frequency-only, ruling 4): the frequency comes from this VTX's own power-up state.
[vtx]
kind = "tramp"
uart = 6
default_band = "R"
default_channel = 1
power_levels_mw = [25, 100, 200, 400, 600]
power_levels_dbm = [14, 20, 23, 26, 28]
default_power_index = 2
reply_latency_ms = 5.0
```

(Keep every other section byte-identical to the freestyle quad — frame, ground, motor, ESC, battery, sensors, fc, osd, radio — and keep the `[osd]` section so the OSD keeps flowing on the same UARTs.)

- [ ] **Step 2: The diff** — `quads/opendrone-5f-tramp.betaflight.diff`, copied from the freestyle diff with the VTX block changed:

```text
# Tramp VTX on UART6 (serial index 5; function mask 4096 = VTX_TRAMP). SITL has no vtxtable: the driver's
# built-in 25/100/200/400/600 mW table applies.
serial 5 4096 115200 57600 0 115200
set vtx_band = 5
set vtx_channel = 1
set vtx_power = 2
```

(every other line of the freestyle diff unchanged — CRSF on UART2, ESC sensor on UART3, OSD on UART4, `motor_pwm_protocol = PWM`, `small_angle`, the aux lines, `force_battery_cell_count`.)

- [ ] **Step 3: The live test** (`python/tests/test_sitl_tramp.py`, new — modelled on `test_sitl_video.py`'s structure: the `skipif` guard on `OFS_SITL_LAUNCH`, the `sim` fixture from `conftest.py`, and MSP over Betaflight's UART1 port with the sim advanced while waiting; copy the `Msp` class from `test_sitl_video.py`)

```python
"""The Tramp VTX through the API against Betaflight SITL (spec 2026-10-10-m3e §6). Needs OFS_SITL_LAUNCH."""
import os
import pathlib

import pytest

import ofs
from conftest import WORLD

pytestmark = pytest.mark.skipif(not os.environ.get("OFS_SITL_LAUNCH"),
                                reason="set OFS_SITL_LAUNCH to run against Betaflight SITL")

QUAD = str(pathlib.Path(__file__).resolve().parents[2] / "quads" / "opendrone-5f-tramp.toml")
MSP_SET_VTX_CONFIG = 89


def test_the_tramp_vtx_follows_betaflight(sim):
    name = sim.load(QUAD, world=WORLD)
    assert "Tramp" in name
    state = sim.state()
    assert state.vtx.present
    assert state.vtx.freq_mhz == 5658, "the VTX's power-up state R1"
    assert state.vtx.power_mw == 200, "default_power_index 2 of the tramp table"
    assert state.video.present, "the video link runs"
    assert state.video.latency_ms == 40, "the flat world has no latency_ms: the default two fields, quantized"
    assert state.video.field_valid, "a clean link at the pad completes every field"

    # Betaflight's init: it queries 'r' then 'v' and adopts the VTX's frequency.
    sim.run(seconds=3.0)
    state = sim.state()
    assert state.vtx.freq_mhz == 5658, "the FC adopted the VTX's frequency (no band/channel on tramp)"

    # MSP_SET_VTX_CONFIG with a frequency: Betaflight sends 'F'; the simulated VTX follows.
    sim.msp_send(MSP_SET_VTX_CONFIG, __import__("struct").pack("<H", 5800))
    sim.run(seconds=2.0)
    state = sim.state()
    assert state.vtx.freq_mhz == 5800, "the 'F' command reached the model"
    assert state.video.present, "the link retuned, not died"
```

Use the exact MSP plumbing `test_sitl_video.py` already provides (its `Msp` class + the way it sends `MSP_SET_VTX_CONFIG` for the SmartAudio channel-change test) rather than `sim.msp_send` if that helper does not exist — read `test_sitl_video.py`'s VTX test and mirror its call shape; the assertions above are the contract. The power change via MSP (index → mW) can be added the same way if the SmartAudio test already drives one; otherwise frequency alone is the pin (power-by-MSP on SITL goes through the same vtx_common path, and the codec's `SetPowerMw` is unit-tested in Task 6).

- [ ] **Step 4: Run it live**

Run (inside WSL, per the Global Constraints recipe): `PYTHONPATH=python uv run --with grpcio,protobuf,pytest pytest python/tests/test_sitl_tramp.py -v` with `OFS_SIM_BIN` pointing at a Linux-built `ofs-sim` and `OFS_SITL_LAUNCH` at the pinned SITL.
Expected: PASS.

- [ ] **Step 5: CI check**

Run: `grep -n "pytest" .github/workflows/ci.yml`
Expected: the sitl job runs `python -m pytest python/tests -v` (whole directory) — **no ci.yml change is needed**; the new file self-skips without `OFS_SITL_LAUNCH` and runs inside the sitl job. Confirm by reading the job's pytest step.

- [ ] **Step 6: Commit**

```bash
git add quads/opendrone-5f-tramp.toml quads/opendrone-5f-tramp.betaflight.diff python/tests/test_sitl_tramp.py
git commit -m "feat(quads): the Tramp live-test quad and its live test"
```

---

### Task 8: Font page 1

The atlas grows a second page (default + bold); `osd.gd` draws the page bits it already receives; arriving pages beyond what's shipped fall back to page 0. The font download needs the user's approval first — the M3a discipline.

**Files:**
- Modify: `tools/make_osd_font.py`
- Modify: `godot/ui/osd.gd`
- Modify: `godot/ui/osd_font.png` (regenerated)
- Modify: `godot/ui/OSD_FONT_LICENSE.md`
- Modify: `godot/tests/test_osd.gd`

**Interfaces:**
- Produces: `make_osd_font.py <default.mcm> <out.png> [page1.mcm]` (a 16×32-glyph, 192×576 px atlas when the second page is given); `osd.gd` `FONT_PAGES: int`, `static func glyph_rect(page: int, code: int) -> Rect2` (page ≥ `FONT_PAGES` → page 0); test `test_osd.gd` pins.

- [ ] **Step 1: Ask the user to approve the font download** — in these words:

> "To draw Betaflight's bold OSD page I need its second font. I want to download `resources/osd/2/bold.mcm` from the official Betaflight Configurator repository (the same place `default.mcm` came from in M3a, GPL-3.0), convert it into the atlas, and commit only the PNG, the converter change and the license-notice update. OK?"

**Wait for a yes.** If the user declines, stop this task and record the decline in `docs/superpowers/m3e-carried-debt.md` (Task 10); the rest of M3e is unaffected.

- [ ] **Step 2: Download both pages and record the provenance**

The source `.mcm` files are not committed (only the atlas PNG is), so both pages are fetched:

```bash
SHA=$(gh api repos/betaflight/betaflight-configurator/commits/master --jq .sha)
gh api "repos/betaflight/betaflight-configurator/contents/resources/osd/2/default.mcm?ref=$SHA" --jq .content | base64 -d > /tmp/default.mcm
gh api "repos/betaflight/betaflight-configurator/contents/resources/osd/2/bold.mcm?ref=$SHA" --jq .content | base64 -d > /tmp/bold.mcm
wc -c /tmp/default.mcm /tmp/bold.mcm; sha256sum /tmp/default.mcm /tmp/bold.mcm
```

Compare `/tmp/default.mcm`'s SHA-256 against the one recorded in `OSD_FONT_LICENSE.md` from M3a (it must match — a changed font would silently alter every glyph). The conversion itself runs after Step 3:

```bash
python tools/make_osd_font.py /tmp/default.mcm godot/ui/osd_font.png /tmp/bold.mcm
```

Record both SHA-256s in `OSD_FONT_LICENSE.md` (Step 5).

- [ ] **Step 3: The converter gains a second page** (`tools/make_osd_font.py`)

Update the docstring:

```python
"""Converts Betaflight OSD font(s) (.mcm, MAX7456 format) into a glyph atlas PNG for the Godot client.

An .mcm file is the text line `MAX7456` followed by 256 glyphs x 64 lines of 8 binary digits. Each glyph is 12 x 18
pixels, 2 bits per pixel, 4 pixels per byte starting at the most significant bits, row by row (54 of the 64 bytes
are used): 00 black, 01 transparent, 10 white, 11 transparent. The atlas is 16 glyphs per row (192 px wide,
288 px per page); glyph `code` of page `p` sits at column `code % 16`, row `code // 16 + p * 16`. With a second
.mcm argument the pages stack vertically (two pages: 576 px tall).

Usage: python tools/make_osd_font.py <default.mcm> <out.png> [page1.mcm]
"""
```

and `main()`:

```python
def main():
    if len(sys.argv) not in (3, 4):
        raise SystemExit(__doc__)
    glyphs = [glyph_pixels(g) for g in read_mcm(sys.argv[1])]
    pages = [glyphs]
    if len(sys.argv) == 4:
        pages.append([glyph_pixels(g) for g in read_mcm(sys.argv[3])])
    width, height = COLS * GLYPH_W, (GLYPHS // COLS) * GLYPH_H * len(pages)
    rows = []
    for page in pages:
        for y in range((GLYPHS // COLS) * GLYPH_H):
            rows.append(...)  # the existing row-assembly loop body, unchanged, reading this page's glyphs
    write_png(sys.argv[2], width, height, rows)
```

(preserving the existing row-assembly code exactly — only the page loop and the height change).

- [ ] **Step 4: Run the conversion, then `godot/ui/osd.gd` draws the pages** — first regenerate the atlas with Step 2's conversion command (`python tools/make_osd_font.py /tmp/default.mcm godot/ui/osd_font.png /tmp/bold.mcm`), then replace `glyph_rect` (osd.gd:71-73) and the draw call:

```gdscript
## How many font pages the atlas holds (the PNG stacks them vertically, 16 glyph rows of 18 px each).
static func font_pages() -> int:
	return int(FONT.get_size().y / (16.0 * GLYPH_H))


## Where glyph `code` of font `page` is in the atlas (pages beyond the atlas fall back to page 0: Betaflight
## addresses up to four pages, the analog font ships two).
@warning_ignore("integer_division")
static func glyph_rect(page: int, code: int) -> Rect2:
	var p := page if page < font_pages() else 0
	return Rect2((code % ATLAS_COLS) * GLYPH_W, (code / ATLAS_COLS) * GLYPH_H + p * ATLAS_COLS * GLYPH_H, GLYPH_W, GLYPH_H)
```

(the vertical offset of page `p` is `p * 16 * GLYPH_H` pixels — `ATLAS_COLS` is the rows-per-page count because the atlas is square in glyphs.) In `_draw_grid` (osd.gd:107), read the cell's page and pass it:

```gdscript
				_canvas.draw_texture_rect_region(FONT, Rect2(box.position + Vector2(col, row) * cell_size, cell_size), glyph_rect((packed >> 8) & 0x3, packed & 0xFF))
```

- [ ] **Step 5: License note** (`godot/ui/OSD_FONT_LICENSE.md`) — add a paragraph: bold.mcm, its source URL, the pinned commit SHA, its size and SHA-256 from Step 2, same GPL-3.0 (not "or later") note as default.mcm.

- [ ] **Step 6: Update the tests** (`godot/tests/test_osd.gd`)

```gdscript
func test_the_font_atlas_has_the_expected_layout() -> void:
	eq(Osd.FONT.get_size(), Vector2(192, 576), "two pages of 16 x 16 glyphs of 12 x 18 pixels")
	eq(Osd.glyph_rect(0, 0), Rect2(0, 0, 12, 18), "glyph 0, page 0")
	eq(Osd.glyph_rect(0, 65), Rect2(12, 72, 12, 18), "glyph 'A' is column 1, row 4")
	eq(Osd.glyph_rect(1, 0), Rect2(0, 288, 12, 18), "page 1 starts one glyph-block down")
	eq(Osd.glyph_rect(1, 255), Rect2(180, 558, 12, 18), "the last glyph of page 1")
	eq(Osd.glyph_rect(2, 65), Osd.glyph_rect(0, 65), "page 2 falls back to page 0")
	eq(Osd.glyph_rect(3, 0), Osd.glyph_rect(0, 0), "so does page 3")
```

Replace the old single-page layout test with this (the `Rect2` expectations are the same arithmetic as before for page 0). Also assert a page-2 packed cell still draws (the fallback path in `_draw_grid` is exercised by the existing drawn-cell tests if they use page-0 cells; add one `packed` with page bits set to the blink test's frame and re-run).

- [ ] **Step 7: Verify and commit**

Run: `scripts/run-godot-tests.sh`
Expected: the suite passes (the live INFO-severity elements now render bold — `shots.gd` shows it).

```bash
git add tools/make_osd_font.py godot/ui/osd.gd godot/ui/osd_font.png godot/ui/OSD_FONT_LICENSE.md godot/tests/test_osd.gd
git commit -m "feat(godot): the OSD draws font page 1 - the atlas stacks the bold page, higher pages fall back"
```

---

### Task 9: The aspect setting

The picture box follows a setting: 4:3 (new default), 16:9, or window-fill. The SubViewport's internal resolution is fixed for the boxed aspects and matches the window otherwise (and always in chase view).

**Files:**
- Modify: `godot/scripts/settings.gd:9-29, 46-59` (names + defaults)
- Modify: `godot/ui/controls_menu.gd`
- Modify: `godot/scripts/app.gd` (`_layout_fpv`)
- Modify: `godot/tests/test_settings.gd`, `godot/tests/test_controls_menu.gd`

**Interfaces:**
- Consumes: `_layout_fpv` (Task 1), `dvr` (Task 5 — the resize-stop path fires on aspect switches).
- Produces: setting `fpv_aspect` ∈ `"4:3" | "16:9" | "window"` (default `"4:3"`; env `OFS_FPV_ASPECT`, flag `--fpv-aspect`); `controls_menu` signal `aspect_changed(value: String)` + `set_aspect(value)`; internal resolutions 960×720 (4:3), 1280×720 (16:9).

- [ ] **Step 1: The setting** (`godot/scripts/settings.gd`)

Add to `ENV_NAMES` / `FLAG_NAMES`: `"OFS_FPV_ASPECT": "fpv_aspect"` / `"--fpv-aspect": "fpv_aspect"`. Add to `defaults()`: `"fpv_aspect": "4:3",`. (`_coerce` passes strings through unchanged.)

- [ ] **Step 2: The menu control** (`godot/ui/controls_menu.gd`)

Following the file's existing CheckBox/button pattern (the `video_effects` control, controls_menu.gd:101-104):

```gdscript
signal aspect_changed(value: String)

const ASPECTS := ["4:3", "16:9", "window"]
var _aspect: Button = Button.new()
var _aspect_index := 0
```

Build it after the video-effects checkbox: `_aspect.text = "Picture: 4:3"`; `_aspect.pressed.connect(func(): _aspect_index = (_aspect_index + 1) % ASPECTS.size(); _aspect.text = "Picture: %s" % ASPECTS[_aspect_index]; aspect_changed.emit(ASPECTS[_aspect_index]))`; `box.add_child(_aspect)`. Plus, mirroring `set_video_effects` (controls_menu.gd:129-134):

```gdscript
## Shows the aspect setting without emitting `aspect_changed`.
func set_aspect(value: String) -> void:
	_aspect_index = ASPECTS.find(value)
	if _aspect_index < 0:
		_aspect_index = 0
	_aspect.text = "Picture: %s" % ASPECTS[_aspect_index]
```

- [ ] **Step 3: The layout** (`godot/scripts/app.gd`) — the math becomes a static function (testable headless, like `osd.gd`'s old `box_for`), and `_layout_fpv` applies it:

```gdscript
## The FPV box and the viewport's internal resolution for `window_size`. The chase view and the "window"
## aspect fill the window (the viewport at the window's size, today's quality); the boxed aspects center a
## fixed-resolution picture like goggles do.
static func fpv_box_for(window_size: Vector2, aspect: String, chase: bool) -> Dictionary:
	var box := Rect2(Vector2.ZERO, window_size)
	var internal := Vector2i(window_size)
	if not chase and aspect != "window":
		var ratio := 4.0 / 3.0 if aspect == "4:3" else 16.0 / 9.0
		var w: float = minf(window_size.x, window_size.y * ratio)
		box = Rect2((window_size - Vector2(w, w / ratio)) * 0.5, Vector2(w, w / ratio))
		internal = Vector2i(960, 720) if aspect == "4:3" else Vector2i(1280, 720)
	return {"box": box, "internal": internal}


## Sizes the FPV box and the viewport's internal resolution. A size change while the DVR records stops it
## (Review Focus 5): stop before the resize so the comparison sees the old size.
func _layout_fpv() -> void:
	var layout: Dictionary = fpv_box_for(fpv_box.size, settings["fpv_aspect"], chase_view)
	var internal: Vector2i = layout["internal"]
	if dvr != null and dvr.recording and fpv_viewport.size != internal:
		var summary: Dictionary = dvr.stop()
		hud.add_toast("DVR stopped: the picture size changed (%d frames saved)" % summary.get("frames", 0), "warn")
	fpv_texture.position = (layout["box"] as Rect2).position
	fpv_texture.size = (layout["box"] as Rect2).size
	fpv_viewport.size = internal
```

In `_ready`, after the video-effects lines (app.gd:48-50):

```gdscript
	controls_menu.set_aspect(settings["fpv_aspect"])
	controls_menu.aspect_changed.connect(_on_aspect_changed)
```

and the handler beside `_on_video_effects_toggled` (app.gd:143):

```gdscript
func _on_aspect_changed(value: String) -> void:
	settings["fpv_aspect"] = value
	_layout_fpv()
	if not AppSettings.save_value("fpv_aspect", value):
		hud.add_toast("Could not save the Picture setting to %s" % AppSettings.CONFIG_PATH, "warn")
```

`set_chase_view` (app.gd:135) calls `_layout_fpv()` after toggling `chase_view` (the chase view always fills the window).

- [ ] **Step 4: Tests** — `godot/tests/test_settings.gd` gains: the defaults contain `fpv_aspect == "4:3"`; `OFS_FPV_ASPECT=16:9` resolves; `--fpv-aspect=window` resolves (copy the file's existing env/flag test shape). `godot/tests/test_controls_menu.gd` gains: `set_aspect("16:9")` then pressing the button emits `aspect_changed` with `"window"` (the cycle continues from the set value); an unknown value falls back to index 0. The box math (app.gd is preloadable as a script; statics are callable):

```gdscript
const App = preload("res://scripts/app.gd")

func test_the_fpv_box_fits_the_aspect_and_centers() -> void:
	var wide := App.fpv_box_for(Vector2(1600, 800), "4:3", false)
	eq(wide["box"], Rect2(266.666, 0, 1066.666, 800), "a wide window: height-fitted 4:3, centred")
	eq(wide["internal"], Vector2i(960, 720), "the boxed aspect renders at its fixed resolution")
	var tall := App.fpv_box_for(Vector2(600, 900), "16:9", false)
	eq((tall["box"] as Rect2).size, Vector2(600, 337.5), "a tall window: width-fitted 16:9")
	var window_mode := App.fpv_box_for(Vector2(1600, 800), "window", false)
	eq(window_mode["box"], Rect2(0, 0, 1600, 800), "the window aspect fills the window")
	eq(window_mode["internal"], Vector2i(1600, 800), "at the window's own resolution")
	var chase := App.fpv_box_for(Vector2(1600, 800), "4:3", true)
	eq(chase["box"], Rect2(0, 0, 1600, 800), "the chase view always fills the window")
```

- [ ] **Step 5: Verify and commit**

Run: `scripts/run-godot-tests.sh && godot --headless --path godot -s res://tests/e2e_open_loop.gd`
Expected: PASS. Manual check: flip the setting through all three values — the box re-centres; the DVR stops with a toast if recording.

```bash
git add -A
git commit -m "feat(godot): the picture box follows an aspect setting - 4:3 default, 16:9, window"
```

---

### Task 10: Docs, carried debt, final verification

**Files:**
- Modify: `docs/dev-setup.md`
- Modify: `README.md`
- Modify: `docs/research/video-link.md`
- Create: `docs/superpowers/m3e-carried-debt.md`

- [ ] **Step 1: `docs/research/video-link.md`** gains a "Timing" section: the field rate and quantization (`latency_ms` default 40 ms = 2 fields, quantized down; estimate + provenance), the drop model (`field_drop_probability`, `FIELD_DROP_P_MAX = 0.5` between the 6 dB and 3 dB bands, zero outside; estimate), and the client's one-instant composite rule.

- [ ] **Step 2: `docs/dev-setup.md`** gains: the `fpv_aspect` setting row (env/flag) beside `video_effects`; the DVR keybind (D) and the recordings directory (`user://recordings/`); the Tramp quad (`quads/opendrone-5f-tramp.toml`) and how to fly it; the manual check list (latency judder, DVR in VLC, aspect flip, Tramp channel change from the OSD menu).

- [ ] **Step 3: `README.md`** — the feature list gains DVR, the video timing model and the Tramp VTX; the settings table gains `fpv_aspect`.

- [ ] **Step 4: `docs/superpowers/m3e-carried-debt.md`** — following the house format:

```markdown
# M3e carried debt

These items were deliberately left out of M3e, or surfaced while building it. They are listed so M4 planning and the maintainer can pick them up.

## Deferred features
- **Font pages 2 and 3.** Betaflight's warnings arrive at page 2 (severity WARNING → `fontSelection[2]`, the
  identity default) and the client falls back to page 0 for them; only page 1 (INFO) draws bold. A page-2
  mapping (or shipping more pages in the atlas) would make warnings bold like a real HD goggle.
- **The DVR has no size mid-recording.** A viewport resize or aspect switch stops the recording instead of
  re-negotiating the header.
- **The DVR has no clean-feed mode.** It records exactly what the pilot sees (the spec's decision 2).
- **A thermal model.** Tramp's 's' status answers a constant 25 °C, like the KISS ESC frames.
- **`FIELD_PERIOD_S` is duplicated** in `ofs-client::fpv` (ofs-video is not a client dependency); the pairing
  with `ofs-video::link::FIELD_RATE_HZ` is by comment only.
- **The spec's own out-of-scope list:** digital video systems; a clean-feed DVR; audio; image-sequence and
  ffmpeg export options; the VTX-interference fault (M4); a full HD/digital OSD element set; configurable
  receiver thresholds; reflections off objects.
```

- [ ] **Step 5: The full verification suite**

Run: `cargo test --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && python -m pytest python/tests -v && scripts/run-godot-tests.sh && godot --headless --path godot -s res://tests/e2e_open_loop.gd`
Expected: PASS everywhere.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: M3e - the timing model, the DVR and the Tramp quad documented; carried debt recorded"
```
