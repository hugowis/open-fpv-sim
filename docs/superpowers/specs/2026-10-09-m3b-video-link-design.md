# M3b: analog video link and degradation — design

Status: design approved in conversation 2026-10-09, awaiting written-spec review. Parent spec: `2026-10-04-open-fpv-sim-design.md` (§4.2 `ofs-video`, §5.4 return paths, §9 M3). Predecessor: `2026-10-08-m3a-osd-vtx-design.md` (merged into main as c1f3292), whose VTX bus signals and reserved canvas layer 3 this milestone consumes.

## 1. Purpose and scope

M3b completes M3: a 5.8 GHz analog link model turns the VTX state that M3a publishes, and the quad's position and attitude in a world, into per-field signal quality, and a degradation layer in the FPV view turns that quality into a picture that breaks up the way analog video does.

**Success looks like:** flying the Godot client against real Betaflight, the picture stays clean near the pilot, gains grain and sparkles with distance, loses colour, tears, rolls and drops to static. Flying behind a building fades the picture over a few metres. The OSD breaks up with the picture. Raising the VTX power from 25 to 600 mW (OSD menu or Configurator) reaches visibly further; pit mode gives a picture only next to the pilot; moving off the channel of the parked emitter at the edge of the field cleans up the picture. The HUD shows the SNR and the active receiver antenna. The same seed in lockstep gives the same link signals, and a Python script sees the same numbers headless.

### Decisions made in the design conversation

| # | Decision | Alternative rejected |
|---|---|---|
| 1 | Buildings block the video signal, through a **shared world file** that the server loads and Godot draws. | Open field only (no occlusion); Godot ray casts sent to the server (no occlusion headless, the server would not own the model). |
| 2 | **Video link only.** The propagation code is a module in `ofs-video` shaped so the ELRS link can adopt it later. | ELRS RSSI, LQ and loss from the same propagation model in M3b. |
| 3 | **No collision** with world objects in M3b; the world's objects are used only for line of sight. | Drone collides with buildings, gates and pylons (touches the M1 contact model). |
| 4 | **Configurable antennas** as data: the world file lists the receiver antennas (default: omni plus patch on diversity goggles), the quad file's `[vtx]` describes the VTX antenna. | One fixed omni; fixed omni + patch. |
| 5 | **Other emitters** in the world file add adjacent-channel interference; the shipped world has one on a channel next to the default R1. | No interference until M4; the mechanism with none shipped. |
| 6 | **The server owns the world and the whole receiver model** (signal, interference, SNR, diversity, sync); Godot asks for the world over gRPC and only draws, and the shader only renders the quality values. | Godot and the server each parse the TOML; the server publishes raw RF numbers and Godot does the receiver logic (not testable headless). |

### Out of scope

Collision with world objects; ELRS on the shared propagation; non-flat terrain; rotated boxes; configurable receiver thresholds (named constants in M3b); the M4 "VTX interference" fault (it will inject emitters at run time); digital video; video latency and frame drops; DVR recording.

## 2. The world file

**`worlds/flat.toml`** (new directory), `schema_version = 1`, loaded by `ofs-config` with the quad file's rules: units in the key suffixes, every validation error collected with file and field, non-finite numbers rejected. Positions are in the quad file's frame, NED metres from home (so "up" is a negative `d`); object sizes are `[north, east, height]`.

- `[pilot]`
  - `position_ned_m`: where the goggles are (head height included).
  - `facing_deg`: the heading the pilot looks along; patch aims are relative to it.
- `[receiver]`
  - `noise_floor_dbm` (default −93: thermal noise in about 20 MHz plus an 8 dB noise figure).
  - `diversity` (default true).
  - `[[receiver.antennas]]`: `name` (unique), `kind` (`omni` or `patch`), `gain_dbi`, `beamwidth_deg` (patch only, required for a patch), `polarization` (`rhcp`, `lhcp` or `linear`), `aim_az_deg` and `aim_el_deg` relative to the pilot's facing. Shipped: `omni` (2 dBi RHCP) and `patch` (8 dBi RHCP, 60°, aimed straight ahead).
- `[[objects]]`: `name` (unique), `shape` (`box` or vertical `cylinder`), `center_ned_m`, `size_m` (box) or `radius_m` and `height_m` (cylinder), `color` (`[r, g, b]`, 0–1), `rf_loss_db` (the loss when the object fully blocks the path; default 0, meaning transparent). Boxes are axis-aligned. The shipped world carries today's launch pad, pylons, gates and buildings from `godot/world/world.gd`, same positions and colours; buildings 20–30 dB, posts and the pad 0.
- `[[emitters]]`: `name` (unique), `position_ned_m`, `freq_mhz` or `band` + `channel`, `power_mw`, and an omni antenna (`gain_dbi`, `polarization`). Shipped: one parked quad at the edge of the field on a channel next to R1.

**The quad file's `[vtx]`** gains two optional fields (the quad schema stays 3):
- `antenna`: `kind`, `gain_dbi`, `polarization`, `mount_frd` (the antenna axis in the body frame). Default: a 2 dBi RHCP omni pointing up and back, as on a typical 5" quad.
- `pit_power_mw` (default 0.1, an estimate): the power while `vtx.pit_mode` is set.

**Loading:** `LoadRequest.world_path`. Empty means the built-in open field: the pilot at home at 1.7 m, one 2 dBi RHCP omni, no objects, no emitters, so existing scripts keep working and the video link still runs. Relative paths resolve like `quad_path`. Godot's settings default to `worlds/flat.toml`.

**Validation (config errors at load):** a missing or unparsable file; an unknown `schema_version`; non-finite numbers; duplicate antenna, object or emitter names; an unknown kind, shape or polarization; a patch without `beamwidth_deg`; a non-positive size, radius, height, power or beamwidth; a pilot below the ground; an emitter frequency outside 5600–6000 MHz or an unknown band/channel; a quad with `[vtx]` whose `sim.base_hz` is not a multiple of 50.

## 3. The link model

`ofs-video::link`: a `VideoLink` model, deterministic, with its own seeded RNG stream. The propagation maths are pure functions in `ofs-video::propagation` so ELRS can reuse them later.

**Rate:** 50 Hz, one PAL field (`base_hz / 50`, 160 at 8 kHz). Inputs from the bus: `body.pos_ned_m`, `body.att_q`, and M3a's `vtx.present`, `vtx.freq_mhz`, `vtx.power_mw`, `vtx.pit_mode`. The model exists only when the quad has `[vtx]`; it transmits from load onward, as M3a's VTX does (`pit_power_mw` replaces `vtx.power_mw` in pit mode).

**Received power, for each receiver antenna:**
`P = Ptx + Gtx(dir) + Grx(dir) − FSPL(d, f) − polarization loss − body shadow − obstruction`, combined with the ground bounce and the fading term.
- **Free-space path loss:** `20·log10(d_m) + 20·log10(f_MHz) − 27.55`, distance clamped at 1 m.
- **Antenna patterns:** an omni is a dipole-like doughnut, `gain + 20·log10(sin θ)` from its axis, floored at −20 dB below the peak; a patch is a cos^n main lobe with n set so that the gain is −3 dB at half the beamwidth, floored at a −20 dB back lobe. The VTX direction uses the quad's attitude and `mount_frd`; the receiver direction uses the pilot's facing and each antenna's aim.
- **Polarization:** same-hand circular 0 dB; opposite-hand circular 20 dB; circular to linear 3 dB; linear to linear `−20·log10|cos Δ|`, capped at 20 dB, where Δ is the angle between the two antenna axes projected onto the plane across the path.
- **Body shadow:** up to 8 dB (named constant) when the path leaves the quad through its frame and battery, scaled smoothly by how far the direction to the receiver points below the body plane and forward.
- **Obstruction (knife-edge, ITU-R P.526):** for each object with `rf_loss_db > 0`, the straight path is sampled (32 points) against the object's signed distance; the deepest point relative to the first Fresnel zone gives the parameter v, the loss is `J(v) = 6.9 + 20·log10(√((v−0.1)²+1) + v − 0.1)` for v > −0.78 (else 0), capped at the object's `rf_loss_db`. Losses of several objects add. Going behind a building therefore fades over a few metres instead of switching off.
- **Ground bounce:** two-ray model over the flat ground (reflection coefficient −1, grazing). Reflection flips circular handedness, so circular antennas take the reflected ray with the cross-polarization loss, which is why FPV uses circular polarization; with linear antennas the bounce makes deep fades when flying low.
- **Fading:** a seeded Rician process per receiver antenna (AR(1) complex Gaussian), K-factor high with line of sight and near 0 (Rayleigh-like) when the path is obstructed, correlated over the distance the quad has moved (correlation length about λ/2, 2.6 cm), so a hovering quad is steady and fast flight flickers.

**Noise and interference:** each emitter goes through the same propagation (no fading), then the receiver's adjacent-channel rejection by frequency offset: 0 dB at 0 MHz, 10 dB at 20 MHz, 25 dB at 40 MHz, 40 dB at 60 MHz and beyond, linear in between (estimated, recorded as such). Per antenna: `SNR = P − 10·log10(10^(noise_floor/10) + Σ 10^(I/10))`.

**Diversity:** the receiver uses the antenna with the best SNR, switching only when another antenna is better by 2 dB. Without diversity, the first antenna is used.

**Receiver behaviour** (analog FM threshold effect; named constants with provenance "estimated"):

| SNR | Picture |
|---|---|
| ≥ 25 dB | clean |
| 25 → 12 dB | grain rises (`noise` 0 → 0.5) |
| < 12 dB | sparkles appear and grow quickly (`sparkles` 0 → 1 at 4 dB) |
| < 8 dB | colour fades (`chroma` 1 → 0 at 5 dB) |
| < 6 dB | sync unstable: tearing, line jitter |
| < 3 dB for 3 consecutive fields | sync lost: rolling; `noise` → 1 (static) at or below 0 dB |
| back ≥ 6 dB for 5 consecutive fields | sync relocks |

**Range check (a unit test):** 25 mW (14 dBm) with 2 dBi RHCP omnis at both ends at their peak gain, noise floor −93 dBm, with body shadow, ground bounce and fading off, falls to 8 dB SNR at 580 m (±5 %); 600 mW reaches about 3 km.

**Bus signals (outputs):** `video.snr_db`, `video.rssi_dbm.<antenna name>`, `video.antenna` (index of the active antenna), `video.interference_dbm`, `video.noise`, `video.sparkles`, `video.chroma` (each 0–1), `video.sync` (0 locked, 1 unstable, 2 lost).

## 4. Protocol and clients

**Protocol 4** (`proto/ofs/v1/sim.proto`, `PROTOCOL_VERSION = 4`; all clients move together):
- `LoadRequest.world_path` (empty: the built-in open field).
- `rpc GetWorld(Empty) returns (World)`: the loaded, validated world: pilot (position, facing), receiver antennas (name, kind, gain, polarization, aim), objects (name, shape, center, size or radius and height, colour), emitters (name, position, frequency, power). Godot never parses TOML.
- `State.video`, a `VideoLink` message: `present` (false without `[vtx]`), `snr_db`, `interference_dbm`, `repeated AntennaRssi rssi` (name, dBm), `active_antenna`, `noise`, `sparkles`, `chroma`, `sync` (`VIDEO_SYNC_LOCKED`, `VIDEO_SYNC_UNSTABLE`, `VIDEO_SYNC_LOST`). It updates at 50 Hz inside the existing state stream; no new stream.
- New events `EVENT_KIND_VIDEO_LOST = 11` (sync lost) and `EVENT_KIND_VIDEO_RESTORED = 12` (relocked), whose message gives the SNR and the active antenna.

**Python:** `sim.load(quad, world="worlds/flat.toml")`, `sim.get_world()`, `state.video` (fields above, `rssi` as a dict by antenna name), the new event kinds.

**`ofs-client` and the `OfsClient` node:**
- The world is fetched once after each successful load and kept until the next load. `get_world()` returns it as a Dictionary **already in Godot's frame** (positions and sizes converted with the existing `frames` conversion); `get_world_version()` changes on each load.
- Telemetry gains `video_present`, `video_snr_db`, `video_sync`, `video_noise`, `video_sparkles`, `video_chroma`, `video_antenna` (name), `video_rssi` (Dictionary by antenna name).
- Settings gain `world_path` (default `worlds/flat.toml`). A missing or invalid world file is a load error shown on the HUD, like a bad quad file.

**HUD:** a video line under the VTX line, e.g. `VID 18 dB  patch  −71 dBm`, and toasts for `VIDEO_LOST` and `VIDEO_RESTORED`. The HUD line is shown in the chase view too.

## 5. Godot

**World builder (`godot/world/world.gd`):** the sky, sun and ground grid stay as they are. The launch pad, pylons, gates and buildings are built from `OfsClient.get_world()` and rebuilt when `get_world_version()` changes (old nodes removed). New chase-view markers: a small tripod figure at the pilot spot with an arrow per patch aim, and a post labelled with its frequency for each emitter. Before the first load only the ground is drawn.

**Degradation layer (`godot/ui/video.gd`, `godot/ui/video.gdshader`, `CanvasLayer` 3, the slot reserved by M3a):** it reads the screen below (scene, lens and OSD), so the OSD breaks up with the picture.
- Grain: luminance noise of amount `noise`, reseeded every rendered frame.
- Sparkles: short horizontal white or black streaks at random positions, density `sparkles`.
- Colour: saturation follows `chroma`, with slight horizontal colour bleed as it drops.
- Sync unstable: sideways line jitter and a tear band drifting down the picture.
- Sync lost: vertical roll blending into full static.
- Clean link: pass-through.
- Photosensitivity: no full-screen bright flashes; static is fine-grained noise around mid-grey; rolling is a smooth scroll.

**When the layer is active:** FPV view only (off in the chase view, like the lens and the OSD); off when `video_present` is false (a quad without `[vtx]` flies with the clean M2 picture); off when the new **"Video effects"** setting is off (controls menu, default on, saved with the other settings; the HUD video line stays).

**Testability:** the mapping from telemetry to shader uniforms is a pure GDScript function.

## 6. Order of work, failure handling and tests

**Order, riskiest first:**
1. `ofs-video::propagation` and the receiver mapping as pure functions with analytic unit tests, including the range check (realism tuning first).
2. `ofs-config`: the world file (schema, validation, the shipped `worlds/flat.toml` copied from `world.gd`), the `[vtx]` antenna and pit power fields; the `VideoLink` model wired into the vehicle; bus signals.
3. Protocol 4, `GetWorld`, the events, the server and Python.
4. `ofs-client`, `ofs-godot`, the world builder, the degradation layer, the setting, the HUD line.
5. CI, docs and the user's manual flight check.

**Failure handling:** the world validation errors of §2 are config errors at load. A non-finite link value is caught by the existing per-step check and stops the simulator. Video loss is drone behaviour: it raises events, never errors.

**Tests:**
- **Unit (analytic):** FSPL at 100 m and 5800 MHz is 87.7 dB; the omni null and peak; the patch at −3 dB at half its beamwidth; the polarization table; knife-edge J(0) = 6 dB (±0.1), about 0 for a clear path, capped at `rf_loss_db`; circular polarization suppresses the ground bounce; the rejection curve; diversity hysteresis; the receiver thresholds; sync lost after 3 fields and relocked after 5; same seed gives the same fading sequence; a hovering quad gives a steady signal; the range check.
- **Config:** world validation errors collected; the shipped world loads; the built-in open field; the quad antenna and pit power defaults.
- **Simulator, open loop (no Betaflight):** the quad near the pilot is clean; behind building B it degrades; at 2 km it loses sync and raises `VIDEO_LOST`; two lockstep runs give identical video signals; `GetWorld` round-trips over gRPC.
- **Live SITL (`python/tests/test_sitl_video.py`, the M3a-to-M3b chain):** at a fixed spot, 25 → 600 mW via `MSP_SET_VTX_CONFIG` raises the SNR by about 14 dB; pit mode drops it below the static threshold; moving off the emitter's neighbouring channel lowers the interference.
- **Godot:** the uniform mapping per sync state, chase view, no VTX and the setting; the world builder from a Dictionary (node count, positions); the open-loop e2e (world built from the server, HUD video line); the screenshot script gains a degraded shot and a before/after comparison of the world.
- **CI:** no new jobs.

**Docs:** `docs/research/video-link.md` (the model's equations and every constant with its provenance, for the education audience); `dev-setup.md` (world files, the "Video effects" setting); `README.md`; carried debt: collision with world objects, ELRS on the shared propagation, configurable receiver constants, non-flat terrain, rotated boxes, the M4 interference fault.

## 7. Risks

| Risk | Mitigation |
|---|---|
| The estimated constants feel wrong in flight. | They live in one module with provenance; the range test pins the link budget; the user's manual flight check judges the feel. |
| Moving the world into a data file shifts the visuals. | The shipped file is copied from `world.gd`; a before/after screenshot comparison. |
| Shader cost on weak GPUs. | One full-screen pass, comparable to the lens. |
| Obstruction sampling misses thin objects. | Thin posts carry `rf_loss_db = 0`; sampling density is a named constant tested against a building-sized box. |
| Fading makes lockstep runs differ. | The fading RNG is a seeded model stream; the determinism test covers the video signals. |
