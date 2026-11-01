# Changelog

All notable changes, by release of the Python package `open-fpv-sim` (the simulator is built from this
repository). The format follows Keep a Changelog.

## Unreleased

### A real 3D model for the quad (#130)

#### Added
- **`godot/drone/quad.gltf`**: the drone in the chase camera is a glTF model now, not boxes drawn in code —
  generated from the part files' geometry by `tools/make_quad_model.py` (provenance and the node-name contract
  in `godot/drone/QUAD_MODEL.md`), instantiated at runtime through Godot's glTF loader. The FPV view is
  unchanged: the camera mount stays inside the camera's near clip at every scale.
- **Propellers turn at each motor's own speed** (the state's `motor_rpm`, on the wire since the flight audio):
  a stopped motor stops, a windmilling descent turns, and above 3000 RPM the blade hides and the blur disc
  stands in.
- **The model scales with the quad's frame** — `motor_positions_frd_m` of the frame part, read client-side: the
  toothpick reads small, the 7" long-range big.
- **A turning preview of the selected quad in the main menu** (`godot/ui/quad_preview.gd`), the unit the
  workbench (#128) will re-host.

#### Changed
- `drone.gd`'s `set_motors` takes motor speeds in RPM, not motor commands; the model-building code moved to the
  generator. No protocol, server or Rust change.

### The stabilized start (#172)
#### Fixed
- **The menu's Start no longer refuses an `open_loop_stabilized` quad under an open-loop setting.** The start asks
  the server for the built-in stabilizer (`Load`'s `stabilize`, already on the wire since protocol 12) when the
  chosen quad file's `[fc] kind` says the quad is its own stabilized FC — so the doc's promise ("flies stabilized in
  Godot just as in Python") holds under every controller pick, menu or menu-less boot. The Godot bridge's `start`
  now accepts a `stabilize` key. No protocol, settings or simulator change.

### Site generator (tools/sitegen)
#### Added
- **`tools/sitegen`**: a standard-library Python tool (3.11 or newer) that turns a recipe (TOML) and an OpenStreetMap
  snapshot into a `.glb` and a world file the mesh-world loader reads. `python -m sitegen fetch <recipe>` downloads the
  snapshot once from Overpass (descriptive `User-Agent`, retries on 429, 502 and 504, never overwrites a good snapshot);
  `python -m sitegen build <recipe>` extrudes the building footprints, adds parametric landmarks (an `arch` vault with end
  doors, a banded `stack`, `walls` with openings and an optional flat roof) and prints a report with the footprints it
  skipped and why. Recipe schema, rules and limits: `tools/sitegen/README.md`.
- **Deterministic output**: the same recipe and snapshot give byte-identical files. The goldens `tools/sitegen/tests/golden/`
  are pinned by the Python tests and loaded by the engine's own loader in `crates/ofs-config/tests/sitegen_golden.rs`.
- **CI** runs the generator's tests in the `core` and `windows` jobs. No network, no new dependency.
- **`docs/research/sitegen-licensing.md`**: what the ODbL asks of an OSM extract in this GPLv3 repository, how the generator
  satisfies it, and the fallback. It is the implementer's reading, not legal advice.

#### Notes
- No real snapshot is committed: the owner confirms the licence note first (status: unconfirmed).
- Not built: pitched roofs, a cooling-tower builder, elevation data, textures, relations and multipolygons, `col_`
  collision proxies. Ground is flat; a stack is open at both ends.
- The engine changes only in the `.glb` loader, which now refuses a short byteStride or an out-of-range offset instead of panicking in a debug build.
### Triangle-mesh worlds (protocol 17)
#### Added
- **`[[meshes]]` and `[[materials]]` in the world file**: a `.glb` placed at `position_ned_m` and `heading_deg`, solid to
  the quad and in the way of the radio. A material is the glTF material's name with an `rf_loss_db`. Caps (estimates): 8
  meshes, 400 000 triangles per world, 64 MiB per file. `worlds/hall.toml` is the demo (a concrete hall with a doorway and
  a glass window, `worlds/assets/hall.glb`); a node named `col_...` is collision and radio only and is not drawn.
  Layout, units and limits: `docs/dev-setup.md`.
- **`ofs_core::mesh::TriMesh`**: triangles with a deterministic BVH, a sphere query and a segment-crossing query.
- **Collision with meshes**: the quad's spheres collide with the surface (two-sided, no inside, so a mesh need not be
  watertight), the landing points with the nearest triangle only, through a 5 mm skin. Events read `HIT <mesh name>`. The
  maths, the tunnelling bound (160 to 320 m/s at 8 kHz) and a timing record (100 352 triangles: 0.102 s against 0.011 s for
  10 s of flight) are in `docs/research/collision.md`.
- **The links lose the `rf_loss_db` of every surface they cross** (video and ExpressLRS): no diffraction around edges, no
  reflection, no thickness.
- **Repro bundles carry a world's mesh files** under `worlds/`.
- **Protocol 17**: `World.meshes` (`message Mesh`: `name`, absolute `path` on the simulator's machine, `position_ned_m`,
  `heading_deg`, and the `digest` of the `.glb`, its FNV-1a 64 hash). The protocol never carries mesh bytes: the Godot
  client loads the file itself. A world without `[[meshes]]` sends none, and nothing else changes for it.
- **`ofs_client::WorldMesh`** (`World.meshes`, the position in Godot's frame) and `WorldMesh::verify()`, which reads the
  file and names the path and the two digests (or the read error) when it is not the server's.
- **`OfsClient.get_world()["meshes"]`**: an Array of {`name`, `path`, `position`, `heading_deg`, `digest_ok`, `error`}
  (empty without meshes).

#### Notes
- A world without `[[meshes]]` is bit-identical to before, and the protocol sends it no meshes.
- The simulator and the Godot client must share a filesystem: the client loads the `.glb` itself and draws nothing for a
  mesh whose file differs from the simulator's digest (`get_world()["meshes"]` says why).
- A mesh with a non-zero `heading_deg` is turned with libm `sin` and `cos` at load, so only `heading_deg = 0` meshes are
  claimed bit-identical across operating systems.
- The Godot drawing of meshes was verified headless only.

### Audio in the DVR (#159, part 2 of 2 of #36)
#### Added
- **The MP4 export has sound.** The flight sound of #158 is synthesised offline from what the recording saw and muxed as
  an AAC track (128 kb/s, mono, 44.1 kHz). The audio is as long as the recorded frames (to one AAC frame).
- **A `<name>.audio` file beside each AVI** (`REC_<ts>.audio`, `REC_<ts>_clean.audio`): while a feed records, the client
  logs the audio inputs, one `F` line per frame the writer accepted and one `C <event_time_s>` line per collision event. Text, LF,
  `ofs-dvr-audio 1` header; an `F` line is `F <state_time_s> <collision_speed_mps> <airspeed_mps> <descent_mps> <n>
  <rpm_1..rpm_n> <cmd_1..cmd_n>` (the n rpms, then the n commands); the file is never modified after the recording
  stops. A crash costs at worst the last second or so of audio (the log is buffered), ending in a truncated line that is
  ignored, and the clip ends at the shorter of audio and video.
- **The export** replays that file through `ofs_audio::Audio` into a temporary raw PCM file next to the output
  (`.<name>.pcm-<pid>.tmp`, removed after the export) and ffmpeg muxes it with `-f s16le -ar 44100 -ac 1 -i <pcm>`, `-c:a aac
  -b:a 128k` and `-shortest`.
- `Telemetry::audio_input` is the one place the telemetry becomes an audio input; the live sound and the DVR both use it.

#### Notes
- **Silent cases**: a recording with no `.audio` file (made before this change, or a hand-made AVI) or with no frames in
  it exports a silent MP4, as before. A malformed `.audio` file fails the export (no MP4; the AVI and the `.audio` file
  are kept; delete the `.audio` file for a silent MP4). The image-sequence export is unchanged and silent.
- **Needs** an ffmpeg with an AAC encoder (its native `aac`) besides libx264, on `PATH`.
- **Determinism**: the PCM is a pure function of the `.audio` file on one platform, not byte-identical across operating
  systems (`sin`, `exp` and `tanh` come from the platform's libm); the AAC bytes also depend on the ffmpeg build. No golden
  audio is stored.
- **Known limits**: the sound leads the picture by the goggles' latency (40 ms by default), because it is sampled from the
  newest telemetry when a frame is written. It ignores the live "Sound" setting (muting the speakers does not silence the
  recording). There is no VTX buzzer: the sim has no buzzer or beeper model, so the track is the flight sound only. The
  temporary PCM file is about 26 MB for a five-minute flight (about 320 MB for an hour).
- Server, protocol and lockstep digest are unchanged by this change.
- Tests: the log round trip is exact; replaying one `.audio` file twice gives identical PCM; the PCM length is 882 samples
  per frame; a truncated last line is tolerated and a bad line is an error; the export arguments carry the audio inputs
  only when asked. With the WSL ffmpeg, by hand (not in CI): an exported MP4 has an AAC stream of the video's length, and
  two exports of one recording give the same audio md5 (same machine and ffmpeg build).
- Not checked: nobody has listened to an exported clip (headless, no audio device). Play an MP4 in VLC: the sound should
  match the picture.

### Terrain in the Godot world (#155, part 3 of 3 of #27)
#### Added
- **The Godot client draws the terrain the physics samples.** Protocol 16 carries the baked grid in `World.terrain`
  (origin, cell size, node counts, heights, digest); the client rebuilds its digest, and a grid that does not match fails
  the load with a protocol error; a verified grid is turned into the mesh by the client (`ofs_client::terrain_mesh`:
  vertices, normals, indices, cells split along the same diagonal as the physics) and `world.gd` draws it with a flat ring out to the horizon and hides the
  flat ground plane. Emitter posts stand on the ground under them (`ground_y`).
- **`OfsClient.get_world()["terrain"]`** (origin, cell size, node counts, vertices, normals, indices, digest) and
  **`OfsClient.terrain_height(x, z)`**, the physics' height at a Godot position, the reference the drawn mesh is
  compared to.
- **A `terrain` Godot e2e suite** (`scripts/run-godot-tests.sh terrain`): the drawn mesh equals the sim's height at the
  nodes and between them, over `worlds/hills.toml`. `shots.gd` also saves `terrain.png`.
- **The park has relief**: `worlds/park.toml` gets a `[terrain]` (seed 153, 3.5 m amplitude, 45 m wavelength, 160 m
  square, flat disc around home): gentle swells of about a metre on the course. The course is placed on the ground
  (gates hang about 1.1 m over it, Gate2, rolled, 1.5 m; the walls, posts, pillars and hedge are buried 0.2 m under the lowest ground below them).
- **Python**: the stubs are regenerated and the channel accepts messages of up to 16 MiB. The `World` dataclass does not
  carry the grid yet; the message does.

#### Changed
- **Protocol 16**: `World.terrain` (`message Terrain`). Messages may now be larger than gRPC's 4 MiB default (a fine
  grid can pass it), so `ofs_proto::MAX_RESPONSE_BYTES` is 16 MiB, set on the Rust client and the Python client.
- **`Terrain::digest` hashes the grid's origin too**, so two grids with the same heights at different places differ. The
  default section's pinned grid digest moved from `0xa382_160e_6e73_4cbd` to `0x9d1c_89d7_d6f5_fa1d`; a world without
  `[terrain]` and its bus digest are unchanged.
- The radio and the video link follow the terrain too (#154).

### Terrain in the radio and video line of sight (#154, part 2 of 3 of #27)
#### Added
- **The terrain hides the quad.** The ELRS link and the video link (the goggles' picture and the interference paths) take
  a knife-edge diffraction loss from the heightfield of #153, the same `Terrain` grid the physics samples (one shared
  `Arc`, no copy). The dominant edge is the point of the straight ray that sits deepest under the ground in Fresnel
  terms (the parameter v, found by sampling twice per cell and refining the best sample); its loss is
  `J(v)` of ITU-R P.526, capped at 60 dB (`TERRAIN_RF_LOSS_DB`, an estimate: a ridge is not a solid wall). Behind an 8 m
  ridge 20 m from each end the loss is 36.8 dB at 5.8 GHz and 33.0 dB at 2.4 GHz.
- **Objects stand on the hills.** `rf_loss_db` objects are rooted on the terrain (the ground under their footprint)
  instead of the plane d = 0, so a building on a slope blocks the signal down to the ground, not to a floating base.
- Tests: synthetic ridges in `ofs-rf` (the closed form, the cap, a clear ray, a ray outside the grid, NaN), the ELRS and
  video links against the same link on flat ground, the shared grid, and `crates/ofs-sim/tests/terrain_link.rs`: on
  `worlds/hills.toml` the quad 1.5 m over the ground 110 m out in clear view hears -59.9 dBm and 14.8 dB video SNR, and
  167 m out behind a hill -78.1 dBm and -3.1 dB (the same spot without the relief: -58.5 dBm, 18.0 dB).

#### Changed
- **A world without `[terrain]` is unchanged, bit for bit**: the radio goldens, the bus digest and the flat-world pin
  still pass. No protocol change.
- **Not yet**: until #155 the Godot client still draws a flat ground in a world with `[terrain]`, so you can lose the
  picture behind a hill the picture does not show. Limits: the ground bounce is still the plane d = 0 (not the
  terrain), only the single dominant edge counts (no Deygout or Epstein-Peterson sum over several ridges), the terrain's
  loss adds to the objects', and sampling is capped at 2048 points, so a span over 1024 cells is sampled coarser than half a cell.

### The terrain slope cap decides exactly (the #161 follow-up)
#### Fixed
- **A world within an ulp of the 60 degree terrain limit now validates identically on every OS**: the cap decides
  `slope * slope <= 3` (`tan²60° = 3` exactly, and an IEEE multiply is correctly rounded) instead of comparing
  `atan`-computed degrees — `atan` is a libm call that may differ by an ulp between OSes, so a boundary world could
  pass on one OS and fail on another. `atan` remains, display only, to name the degrees in the rejection message.

### The game's launcher takes an automatic server port (the #110 follow-up)
#### Added
- **`server_addr` port 0 lets the game start beside anything on the fixed port**: with `--server 127.0.0.1:0`
  (the `server address` setting, or `OFS_SERVER_ADDR`) the Godot client's launcher passes `ofs-sim --port-file`
  and connects to the OS-picked port the server reports there, as Python's `ofs.launch()` has since #110 — a
  process holding `127.0.0.1:50051` no longer breaks game launch, and two clients can fly at once. Every fixed
  address behaves exactly as before.

### DVR export: a failed retry and a panicking export (#137)
#### Fixed
- **A failed MP4 export clears its partial file so a retry works, and a panicking export reports instead of going
  silent**: the half-written `<stem>.mp4` a failing ffmpeg left behind no longer makes every retry die on
  "already exists" (the file is removed only when the failed run itself wrote it; the never-overwrite rule stands),
  and a panic in the export thread reaches `dvr_export_poll()` as an error instead of hanging the client's UI state.

### A heightfield ground for the physics (#153, part 1 of 3 of #27)
#### Added
- **`[terrain]` in the world file**, opt-in: `seed`, `size_m`, `cell_m`, `amplitude_m`, `wavelength_m`, `octaves`,
  `persistence` and `flat_radius_m`, every key optional (a bare `[terrain]` is rolling hills). The loader bakes the
  numbers once into a grid of heights, `ofs_core::terrain::Terrain`, which is the one source of truth: the physics
  samples it, and the later client and radio lanes will read the same grid. The ground is a triangle mesh (each cell split
  along one fixed diagonal), exact at the nodes; beyond the grid it is the plane d = 0. The generator is seeded value
  noise using only `+ - * /` and `sqrt`, so a grid is bit-identical on every OS. A world is rejected when its steepest
  triangle passes 60 degrees, or when the pilot or the handset would stand below the ground.
- **`worlds/hills.toml`**: a launch pad on a 30 m flat disc with 12 m hills around it, no objects, no gates: a place to
  land and slide on slopes.
- **`body.terrain_d_m`**, the ground's depth under the body's centre, published in worlds with `[terrain]` only.
- Tests: the baked grid's digest is pinned (one constant, every OS); a quad rests, holds or slides on a 20 degree ramp
  by the friction angle; a seeded flight over `worlds/hills.toml` replays exactly; a world without `[terrain]` reproduces
  a bus digest recorded before the change (Windows and Linux; not pinned on macOS).

#### Changed
- **The ground contacts follow the terrain**: the collision spheres and the landing points' spring-dampers and static
  friction see the surface under them (its height and slope normal) instead of the plane d = 0, so the quad lands, rolls
  and slides on hills. A world without `[terrain]` is unchanged, bit for bit, and so are its signal list and bus
  digest. No protocol change.
- **Cost**: sampling roughly doubles the (tiny) physics cost, about +4 ms of wall time per simulated second on a
  hilly world (release, 8 kHz), still far above real time; the numbers are in `docs/research/collision.md`.
- **Not yet** (at the time): until #155 the Godot client draws a flat ground in a world with `[terrain]`, so the
  picture and the physics disagree there; the radio and the video link ignored the terrain until #154 (done, see above).
### Procedural flight audio (#158)
#### Added
- **The sim has sound.** The Godot client plays the quad's microphone: a tone per motor from its eRPM (the blade-pass
  frequency, louder with the motor's command, so a motor that disagrees with the others is heard as its own pitch), prop
  wash noise that follows the mean command, the spread between motors and the descent, wind noise that rises with airspeed
  (ground speed for now: the sim has no wind yet) and a thud on every collision, louder with the impact speed.
- **A "Sound" entry in Settings** (setting `audio`, `OFS_AUDIO`, `--audio`; on by default). It mutes the `Flight` audio bus
  and persists in `[client]` of `user://ofs_client.cfg` like the other settings.
- **The `ofs-audio` crate**: the pure sound-parameter functions (`params`, `thud_gain`) and a deterministic `Synth`, with no
  Godot and no I/O. Part 2 (#159, the DVR audio track) renders the same synth offline from a recording.
- `OfsClient.get_telemetry()` carries `motor_rpm`; `OfsClient.audio_render(frames, active)` and `audio_sample_rate()` feed
  the generator.

#### Notes
- The sound is derived on the client from the state stream and the collision events. The server does not produce or carry it
  and it is outside the lockstep digest, so recordings and goldens are unchanged. No protocol change (still 15).

### Controller setup: calibration, shaping and profiles (#127)
#### Added
- **Calibration wizard** on the F2 screen: centres, then end points, per axis, once per controller (stored by the
  controller's GUID, or its name when it has none). Calibrated sticks scale each side of the centre on its own, so an
  off-centre radio axis or unequal throws reach exactly -1 and +1; a throttle or switch uses its whole range.
- **Per-axis shaping**: a deadband and a travel slider on roll, pitch and yaw (the global deadzone stays as the default).
  Invert was already there. Rates and expo stay Betaflight's.
- **Named profiles**: pick, create and delete them on the F2 screen. A new one starts from the preset the controller's name
  suggests (gamepad or radio), and so does a first run with a controller connected.
- **A live monitor** on the F2 screen: the stick view and aux squares, and the connected controllers' names.
- **Controls** in the main menu opens the same screen (Esc or B returns), so the handset can be set up before a flight.

#### Changed
- `user://controls.json` is now version 2 (`profiles`, `active`, `calibration`). A version 1 file loads unchanged as the
  profile `Default`; the screen rewrites it as version 2 on the first change. The F2 screen scrolls in a window that is
  shorter than it.

### An iBUS (FlySky) receiver (#8)
#### Added
- **`receiver = "ibus"`** in `[radio]`: a `Receiver` that sends the link's delivered channels as FlySky iBUS frames
  (32 bytes, 14 channels in microseconds, checksum 0xFFFF minus the byte sum) on the flight controller's UART, about every
  7 ms whatever the link's packet rate, and none while the link delivers nothing, so Betaflight's failsafe fires as with
  CRSF. Codec and receiver in `crates/ofs-radio/src/ibus.rs`, tested against a frame computed by hand. The frame period
  and the value range are estimates; `docs/research/ibus-receiver.md` has the sources.
- **`quads/opendrone-5f-ibus.toml`** and its Betaflight diff (`serialrx_provider = IBUS`): the freestyle quad behind a
  FlySky receiver, on a fixed link at 250 Hz (the FlySky air protocol is not modelled).

#### Changed
- **The SITL patch defines `USE_SERIALRX_IBUS`**: the shipped SITL had no iBUS driver. Rebuild it
  (`scripts/build-sitl.sh`) to use an iBUS quad; the new live test reads iBUS sticks back over `MSP_RC`. Other quads fly
  as before.

### Several courses and race modes per world (#132, protocol 15)
#### Added
- **`[[courses]]` in the world file**: a name, an ordered list of declared gate names, an optional `laps` and a `mode`.
  `[race] course` picks the one a session starts in. A world without `[[courses]]` is unchanged: one unnamed course of all
  its gates. Gate numbers (`next_gate`, "Gate1 (gate 1)") count along the course.
- **A time attack** (`mode = "time_attack"`, `time_limit_s`): as many laps as fit in the time; the race ends at exactly the
  deadline with `race_finished`, and a crossing after it counts for nothing.
- **Protocol 15**: `ResetRace` takes a course name (`ResetRaceRequest`; an unknown one is `InvalidArgument`), `RaceState`
  gains `course`, `course_index`, `mode`, `time_limit_s` and `time_left_s`, `World` gains `courses` and `default_course`;
  the `race_reset` event says `race reset: <course>` in a world with named courses. The repro journal line is
  `race_reset [index]`. New signals `race.course` and `race.start_s` (worlds with gates only).
- **Python**: `ofs.Course`, `World.courses`/`default_course`, `Race.course`/`course_index`/`mode`/`time_limit_s`/
  `time_left_s`, `sim.reset_race(course="")`.
- **Godot**: a Course row in the menu (a world with two or more; `OFS_COURSE`, `--course=`), **Shift+N** cycles the course
  and restarts it (N restarts the same one), the HUD lap line names the course and a time attack counts down, and the
  world draws only the current course's gates.
- **`worlds/park.toml`** gains two courses: "Diamond" (the whole loop, the default, as before) and "Hunt" (the same gates as a
  60 s time attack).
#### Changed
- `ofs-godot`'s `reset_race` takes a course name (an empty String keeps the current course); `ofs-client`'s
  `Command::ResetRace` carries it and `RaceInfo` is no longer `Copy`.

### Game content, first batch: a modelled quad and two new worlds (#130, #131)
#### Added
- **`worlds/park.toml`**: a proximity park, a tight four-gate diamond among walls, pillars and a hedge, made for slow
  airframes. Gate 2 is rolled 15 degrees; a rolled frame cannot be drawn with axis-aligned boxes (#28), so that gate has no
  posts and bar.
- **`worlds/meadow.toml`**: an open meadow, a big fast four-gate loop for the 5" and 7" quads, with trees, a barn and hay
  bales for the video link to block. Gate 2 is pitched 20 degrees (same limit); the diagonal gate 1 gets posts only.
  Both load from the menu and race and reset like `flat` and `circuit`; `crates/ofs-config/tests/world.rs` covers them.

#### Changed
- **The quad in the chase camera is modelled** (plate, arms, motor bells, battery and strap, two antennas, a camera
  housing at the FPV camera's up-tilt) instead of gray boxes, still with each prop turning at its own motor command. The
  FPV view is unchanged: the housing sits inside the camera's near clip. It is built in code (`godot/drone/drone.gd`), not a
  glTF model, and the menu and workbench previews are not part of this batch.

### A compass: the magnetometer model and the heading (#31)
#### Added
- **A magnetometer library part** (`library/magnetometers/<id>.toml`, `[mag]`: `noise_std_ut`, `rate_hz`, `hard_iron_ut`, a
  row-major `soft_iron` matrix, each with provenance). One part ships, `opendrone-mag`, every value `estimated` (no
  magnetometer datasheet could be read, so none is claimed). A quad adds it with `mag = "opendrone-mag"` at **quad schema 7**;
  schema 5 and 6 files load unchanged and the shipped quads stay at 6 with no compass, so no existing flight, recording or
  golden changes.
- **`ofs-sensors::mag`**: the model publishes `mag.field_frd_ut` (µT, body frame: `soft_iron · (Rᵀ · B_earth) + hard_iron + noise`)
  and `mag.heading_rad` (the tilt-compensated magnetic heading) at the part's rate, plus the pure `earth_field_ned`,
  `magnetic_heading` and `true_heading` (declination east positive).
- **The world carries the earth's field**: `[environment]` gains `magnetic_field_ut` (50), `magnetic_inclination_deg` (60, down)
  and `magnetic_declination_deg` (2, east). The defaults have the direction of the field Betaflight SITL's virtual compass
  synthesises, pinned by a test.
#### Known limits
- **Betaflight does not see this magnetometer.** The SITL datagram has no magnetometer field; SITL builds its compass from the
  attitude quaternion and a fixed field. The model feeds recordings, scripts and clients; extending the datagram and
  patching SITL is the follow-up (`docs/superpowers/specs/2026-10-25-compass-design.md`).

### Static friction and a full position correction in the contact model (#65)
#### Changed
- **A landing point sticks.** Its friction was `mu*N*v_t/max(|v_t|, 5 cm/s)`, viscous below 5 cm/s, so a resting quad
  crept down any slope. It is now static Coulomb friction: each touching point keeps a tangential anchor spring (the
  stiffness and damping of its normal spring, no new parameter or schema change) capped at `mu*N`. A quad on a slope
  within the friction angle stays put (a millimetre of sag, then nothing); past it, it slides at `g(sin - mu cos)`.
  A quad sliding on its feet at 1 m/s stops at `mu*g` and stays stopped.
- **The position correction leaves every penetrating contact, not just the deepest.** A sphere in the corner of the
  ground and a wall is out of both in one step (before: the second waited for later steps); contacts whose normals
  partly oppose converge over up to 4 sweeps. A lone contact is corrected exactly as before.
- No golden digest changes (no golden flight touches down or hits anything); a new unit test pins the single-contact
  correction bit-for-bit. Honest limit: a body resting on collision *spheres* on a slope still creeps `g*sin*dt`
  (0.4 mm/s on 20 degrees) because that impulse follows the integration; `docs/research/collision.md` has the model.

### A DVR start never fails on a taken name
#### Fixed
- **A recording started in the same millisecond as another failed with "already exists"** (the `REC_<ts>` stamp has
  millisecond resolution, and D pressed twice in a frame, or a script, reuses it; the open-loop end-to-end hit it about
  one run in three). The DVR now appends `_2`, `_3`, ... to the stem until neither the goggles nor the clean file exists.

### More pre-assembled quads: a toothpick, a race quad and a long-range quad (#129)
#### Added
- **Three quads that fly differently**, each built from library parts with its own Betaflight diff: `opendrone-3t-toothpick`
  (3" 125 mm, 3S 450 mAh, 105 g, thrust-to-weight 4.2, hover at 44 %), `opendrone-5r-race` (5" 220 mm, 6S 1300 mAh,
  540 g, thrust-to-weight 14.8, hover at 19 %, the ICM-42688-P IMU) and `opendrone-7lr-longrange` (7" 320 mm, 6S 4000 mAh
  Li-ion, 900 g, thrust-to-weight 6.3, about 100 W in the hover, a 900 MHz ELRS link). Each loads headless and arms and
  holds 1 m under SITL; the README lists the figures.
- **Fourteen library parts** for them (frames, 1404 / 2306 / 2806.5 motors, 3" / 5" race / 7" props, 20 A and 60 A ESCs, a 3S
  450 and a 6S 4000 pack, a 400 mW VTX). Every value is `estimated` (class-typical, scaled from the 5F parts, none read
  from a datasheet); the ledger `docs/research/component-library.md` lists them.
- **`crates/ofs-config/tests/quads.rs` and `crates/ofs-sim/tests/quads.rs`**: every shipped quad loads and validates, every
  library part has a ledger section, and the three new quads show the thrust-to-weight and hover throttle their files claim.

### A main menu and a settings screen (#123, #126)
#### Added
- **The game opens in a main menu**: choose a world and a quad (the `*.toml` files of `worlds/` and `quads/` in the
  checkout and in the data dir), race or free flight (worlds with gates), and Betaflight or open loop (Betaflight when a
  SITL launch command is set), then Start flying. The choices are the existing `world_path`, `quad_path`, `open_loop`
  settings and a new `race_start`, saved to `ofs_client.cfg` as they change. **Esc** in a flight returns to the menu in the
  same process (the simulator and the Betaflight it started stop; Start launches them again). Keyboard and gamepad
  navigate it.
- **A settings screen** (from the menu) for video effects, picture aspect, HUD shown at start, fullscreen, the DVR feed
  and export, open loop, the simulator binary and address, the data directory and the SITL launch command. It is a view
  of `[client]`: it shows the effective value and locks a row that a flag or an `OFS_*` variable sets this run, naming
  the source. The F2 screen and this one share one apply-and-save path.
- **Settings `menu`, `race_start`, `fullscreen`, `hud_visible`** with `OFS_MENU` / `--menu=`, `OFS_RACE_START` /
  `--race-start=`, `OFS_FULLSCREEN` / `--fullscreen`, `OFS_HUD` / `--hud=`. `--menu=off` boots straight into the flight
  (what the e2e suites use). Free flight (`race_start` off) hides the HUD's race line and the next-gate highlight.

### The IMU and the barometer as library parts (#42)
#### Added
- **`library/imus/` and `library/barometers/`**: `opendrone-imu` and `opendrone-baro` hold the numbers the shipped quads
  carried inline, so the simulation is unchanged, and `tdk-icm-42688p` is an IMU sourced from the ICM-42688-P datasheet's
  noise densities (the conversion to a per-sample deviation is in its provenance and in
  `docs/research/component-library.md`). Their provenance appears in `sim.parts` as the kinds `imu` and `baro`.

#### Changed
- **Quad schema 6**: a quad writes `imu = "<id>"` and `baro = "<id>"` beside `frame = …` instead of the `[imu]` and `[baro]`
  tables; the five shipped quads are converted. Schema-5 quads with inline tables still load (without provenance for
  those two); a part reference at schema 5, or an inline table at schema 6, is a validation error naming the field.
  `PartRefs` gains `imu` and `baro` (`None` for an inline table).

### OSD warnings and criticals (part of #32)
#### Changed
- **The Godot OSD draws Betaflight's warning and critical severities.** Cells on font page 2 (WARNING) and page 3
  (CRITICAL) were drawn like ordinary text; they now use the bold glyphs in amber and red. No new glyph art or licence
  change. The HD/digital element set of #32 is not done: it needs an HD SITL build (see `docs/roadmap.md`).

**DVR bundle (#34, #35, #37):** no protocol, server or Python change. **Rust and Godot API:** `Client::dvr_start`, `dvr_write_frame` and `dvr_stop`
now take a `DvrFeed` (`Goggles` or `Clean`), and the Godot `OfsClient.dvr_start`, `dvr_write_frame` and `dvr_stop` take
the feed name (`"goggles"` or `"clean"`); `OfsClient.dvr_export(path, mode)` and `dvr_export_poll()` are new.

### A DVR that survives a resize (#34)
#### Changed
- **A window resize or an aspect switch no longer stops the recording.** Frames are scaled to fit inside the size the
  recording started with and centred on black bars. Entering the chase view, a reload and a lost session still stop it.

### A clean-feed DVR (#35)
#### Added
- **`dvr_feed` (`goggles`, `clean`, `both`)**, a button on the F2 screen, `OFS_DVR_FEED` and `--dvr-feed=`. `clean`
  records the camera image without the OSD and the video-link effects into `REC_<ts>_clean.avi`; `both` writes it
  beside the goggles file `REC_<ts>.avi`. Both files write on every field (a black clean frame stands in where the
  clean picture does not exist yet), so their frame counts are equal unless the stop toast reports dropped frames. An
  invalid value means `goggles`.

### Image-sequence and MP4 export (#37)
#### Added
- **`dvr_export` (`none`, `frames`, `mp4`, `both`)**, a button on the F2 screen, `OFS_DVR_EXPORT` and `--dvr-export=`.
  After a recording with frames, `frames` copies the AVI's JPEGs unchanged to `<name>_frames/frame_000001.jpg...` and
  `mp4` runs the `ffmpeg` on `PATH` (it needs libx264; without ffmpeg a toast says so and the AVI is kept). Nothing is
  overwritten, and the work runs on a worker thread. An invalid value means `none`.

## 0.4.0 — 2026-10-24

**Upgrade:** protocol 14 (rebuild/reinstall the client and server together); a protocol-13 Python client or server
is refused at the handshake. In a world with gates, a pass now needs room for the quad (a clipped pass raises
`gate_hit` and no longer counts), and the bus digest covers the new `race.finished` signal, so a repro bundle of a
gate world recorded by 0.3.0 no longer reports `reproduced=true` — re-record it on 0.4.0. A bundle's journal can now
hold a `race_reset` line, which a 0.3.0 build refuses to read. Worlds without gates, and every golden, are unchanged.

### A repro bundle keeps the EEPROM as the session failed (#106)
#### Changed
- **`firmware/eeprom.bin` (and `betaflight.diff`) are read when the bundle is written, not at `Load`**, so a setting the
  Configurator saved after `Load` is in the bundle and a replay's Betaflight boots with it. The file is read twice and
  kept when both reads match; a missing or unsettled file falls back to the `Load`-time copy. `bundle.toml` gains
  `firmware_captured` (`"failure"` or `"load"`) for Betaflight sessions. A replay flies the whole journal with the
  failure's settings (`docs/repro-bundle.md`, "The firmware's settings"). Settings changed but not saved never reach the
  EEPROM and are not captured.

### Race gates: the quad's size and tilted openings (#103)
#### Changed
- **A pass has to fit the quad.** The crossing point must be at least the quad's bounding radius (its collision
  spheres' farthest edge, 0.2 m for the shipped quad) inside the opening's edges. A pass whose centre is inside the
  opening but closer to an edge raises the new `gate_hit` event (protocol 14, `EVENT_KIND_GATE_HIT`; Godot toasts it),
  gives no credit and leaves the gate next. A gate counts again, as after any pass, only once the quad has been 3 m
  from its plane. A course whose openings are wider than the quad by more than its radius, like `worlds/circuit.toml`,
  is unaffected.
#### Added
- **`pitch_deg` and `roll_deg` on a `[[gates]]` entry** (default 0, an upright gate; aerospace Z-Y-X after the
  heading). The crossing test, the ground check and the Godot frame use the tilted plane; `Gate.pitch_deg` and
  `roll_deg` on `World.gates` (protocol 14, Python `ofs.Gate`). Worlds without them behave exactly as before.

### Race: lap limit, reset and next-gate highlight (#104)
#### Added
- **`[race] laps = N` in a world file** ends the race after N laps: `lap_completed`, then the new `race_finished`
  event, the clock stops and nothing counts until a reset. `State.race` gains `finished` and `lap_limit`, and the
  `race.finished` signal joins the other `race.*` ones (so a recording of a world with gates has one more channel).
  Worlds without `[race]` race forever, as before.
- **`ResetRace`** (protocol 14; Python `sim.reset_race()`, the **N** key in the game) starts a fresh race at the next
  tick without a `Load`: no clock, laps or times, gate 0 next, a `race_reset` event, the `race.*` signals back to zero.
  A world without gates refuses it. A repro bundle's journal records it (`race_reset`).
- **The game highlights the next gate** (green and stronger; the start/finish gate stays yellow until it is next), shows
  `LAP 2/3` with a limit and `RACE FINISHED` at the end, and toasts both events.

### Part provenance over the API (#43)
#### Added
- **`LoadReply.parts`** (protocol 14) lists the library parts the quad loaded — frame, motors, props, escs, battery
  and vtx, a single motor/prop/esc id once — with the provenance of every parameter (`datasheet`, `measured` or
  `estimated`, the link and the note). Python: `sim.parts` after `load()` (`ofs.Part`, `ofs.Provenance`);
  `part.estimated` names the parameters that are only guesses. Rust: `QuadConfig::part_provenance`.

### Reader error for a still-running recording (#117)
#### Changed
- **`ofs.reader.read` on a file that is still being recorded** says so: the "unfinished recording" error now names the
  running recording as the likely cause and the way out (`sim.recording.stop()`, or `sim.log`, which stops it).

### Tests
- The open-loop Godot end-to-end test waits for a current field instead of judging one frame, and the DVR check owes no
  frames for boundaries held before the first fresh one (#118: the flake was seen on the macOS job only; the issue stays
  open until that job has a clean record).

## 0.3.0 — 2026-10-22

**Upgrade:** protocol 13 (rebuild/reinstall the client and server together); a protocol-12 Python client or server
is refused at the handshake; `OFS_LIBRARY` may now be a list; the inputs CSV gains optional `tx`/`link_loss`
columns; `radio.telemetry_slot_divisor` defaults to 4; a new `DownlinkLoss` fault (the bus digest covers the new
`fault.radio.downlink_loss_fraction` signal, so a repro bundle recorded by 0.2.0 no longer reports
`reproduced=true` — re-record bundles on 0.3.0); the F2 DVR toggle.

The release also carries the work that landed while it was open — parallel SITL sessions (M9a), the experimental
native Windows SITL (M9b), macOS support (M9c), the stabilized-session and hardening sweeps (B2 to B5), and the
audit follow-ups — the detail by item follows.

### The component library is a search path (#44)
#### Changed
- **`OFS_LIBRARY` may list several roots**, `;`-separated on Windows and `:`-separated elsewhere (empty entries are
  dropped; a blank value means the default, the library beside the quads). Each part id resolves against the first
  root that has it, and a part no root has is refused naming every root searched. A repro bundle merges the roots
  the session read at `Load` into its `library/` first-match-wins (a root that has gone missing is skipped), and a
  replay builds the bundle's own library — the replayer's `OFS_LIBRARY` never reaches it.

### A configurable telemetry slot ratio (#22)
#### Added
- **`[radio] telemetry_slot_divisor`** (both link kinds, default 4): one downlink packet in that many carries a
  telemetry frame when heard. It accepts 1, 2, 4, 8, 16, 32, 64 or 128 — there is no off value — and one value
  covers every band and packet rate. It changes the telemetry cadence only: packet loss, uplink delivery and
  downlink LQ are the link model's own. The default 4 keeps 0.2.0 recordings and the radio goldens byte-identical.

### Downlink telemetry loss
#### Added
- **`DownlinkLoss(fraction)`** drops `fraction` of the handset's telemetry packets while leaving the uplink, sticks
  and failsafe untouched. Protocol 13 adds `Fault.downlink_loss` (oneof field 10, `at_s` stays 9) and
  `ofs.faults.DownlinkLoss`; default MCAP recordings leave the new control signal out, so a 0.2.0 no-fault
  recording stays byte-identical.

### Link state in the recorded sticks (#56)
#### Changed
- **The inputs CSV gains optional `tx` and `link_loss` columns** (0/1; with `t_s` and the eight sticks, eleven
  columns in all). A recording writes a row whenever a stick, the transmitter or the link changes — a **K** cut with
  the sticks still is recorded — and `play_inputs` applies both through the same setters a live session uses, so the
  recorded flight round-trips its bus at each tick. A 0.2.0 stick-only file still plays unchanged; a file with the
  new columns is refused by a 0.2.0 server (an unknown column is a header error).

### A DVR toggle in the game (#38)
#### Added
- **The F2 controls screen has a DVR checkbox**, one start/stop path with the **D** key: either one moves the other,
  an automatic stop (a picture-size change, such as a resize) unchecks the box, and the HUD shows `REC` while a
  recording runs.

### Reading a recording while it is written (#48)
#### Added
- The answer, probed and written down (`docs/research/mcap-determinism.md`): a recording's summary section, footer
  and tail magic are written only by `stop()`, so `ofs.reader` refuses the live file ("unfinished recording") and no
  indexed reader can build its view until then — a streaming reader sees only the flushed chunks. Call
  `recording.stop()` (or `sim.log`, which stops and returns it) first. Foxglove itself was not installed on the
  evidence machine, so its behaviour on a live file is unchecked — #117.

### Replaying a bundle no longer runs its programs (#108)
#### Security
- **`ofs-sim --replay` and `ofs.repro.replay_bundle` do not run the programs a bundle's quad file names** (`fc.launch`,
  `fc.cleanup`): a repro bundle is whatever someone attached to an issue. A Betaflight bundle replays with
  `OFS_SITL_LAUNCH` set to your own SITL, and is refused without it; `--trust-bundle` (Python `trust_bundle=True`) runs
  the bundle's own programs. A bundle that asks for more than a simulated day of ticks is refused instead of spinning.

### M9c: macOS (#68)
#### Added
- **macOS survey and support (best effort).** Betaflight SITL builds and runs on macOS with `third_party/betaflight/ofs-sitl-macos.patch`
  (applied by `scripts/build-sitl.sh` on Darwin; the realtime clock replaces `pthread_condattr_setclock`), the slot cleanup
  finds stray SITLs with `ps` where there is no `/proc`, `scripts/run-godot-tests.sh` runs the macOS editor, and CI has a
  `macos` job (Apple silicon: Rust tests, live Betaflight tests, Python, Godot with the game flying Betaflight). The full list of what
  builds and what does not, with run ids: `docs/research/macos.md`. A manual `macos-survey` workflow reruns the survey on
  Apple silicon and Intel.
#### Changed
- Tests that assumed GNU/Linux no longer do (`touch -d`, a one-shot read of the event stream, a tight timing window); one
  test that needs a second loopback address and one that needs a held port to stop SITL are skipped on macOS (the second
  returns early, so it shows as passed); `[dev]` leaves out `rerun-sdk` on Intel Macs, which it has no wheel for.
- The cleanup of a SITL launched without a slot is a `ps`-based script on macOS, where `pkill -x betaflight_SITL` finds nothing.

### Audit follow-ups (2026-10-22, `docs/superpowers/plans/2026-10-22-audit-smaller-fixes.md`)
#### Fixed
- **Two simulators sharing a data folder can no longer collide on a log name**: an MCAP recording, an inputs CSV or a
  repro folder whose number another process took first is created under the next number.
- **`ofs.launch()` no longer picks a port and hopes**: the server binds first (`--listen 127.0.0.1:0`) and reports its
  port through the new `--port-file FILE` option, so a launch cannot end up on another process's server.
- SITL output is read in lines of at most 64 KiB, so a child that prints without newlines cannot grow memory.
#### Added
- A test that the committed Python stubs match `proto/ofs/v1/sim.proto`; Dependabot for the workflows' actions, and the
  publish job's actions pinned to commit SHAs.

### B5: the small-tails sweep
#### Fixed
- **`ofs-validate` reports every `submission.toml` schema problem in one pass** (unknown keys, wrong types, per key),
  instead of stopping at the first; a file that does not parse still says so with its line.
- **Ending a session stops a running MCAP recording with a named `recording_stopped` event**, exactly as a running
  inputs recording does; the file is finished, not left half-written.
- The live SITL tests kill their own session's SITL by slot (the same selection the product's cleanup makes) instead
  of `pkill -x betaflight_SITL`, which took down every SITL on a developer machine; the crash test holds a decoy SITL
  in another slot across the kill to prove the scoping.
- The victim tests' fake SITL processes are killed when a test's assertion fires (a drop guard), so a failing test
  no longer leaks a process holding the ports.
- The release checker's changelog walk catches an empty `## Unreleased` section and a `###` heading outside any
  release; `docs/dev-setup.md`'s protocol-12 line names the stabilizer flags.
#### Removed
- A redundant journal-replay stick-clamp test that passed with the clamp removed (the clamp is pinned by
  `set_sticks_clamps_what_it_accepts`).

### B4: race polish
#### Fixed
- **The HUD's lap clock rounds before it splits minutes**, so a lap of 59.96 s shows `1:00.0`, never `0:60.0`.
- **Race events keep their crossing time inside a `changes()` batch**: the batch is time-ordered (a stable sort, so
  events at the same time keep the order they were raised). A gate event now also has its end-to-end test through
  `Session::changes()`, flying the circuit world on the stabilizer.
- The Godot world node's header, `python/README.md`'s race fields (missing `gate_count`) and the race-timing Python
  snippet's note (an unsteered flight prints no events) say what is true; the M8a plan's ledger rules that the exact
  opening edge of a gate is counted.

### B3: benchmark-gate truth
#### Fixed
- **The ci-linux baseline's "open loop, recording" row is 47.1, not 47.9**, and the five runs agreed within about 6 %,
  not 4 % (the M8c review recomputed the medians; the gate moves 28.7 to 28.3, negligible). The docs now name the
  baseline file instead of a superseded run, and the reader benchmark says "three patterns".
- `scripts/benchmark.py` refuses `--runner`/`--min-ratio` without `--baseline` (they were silently ignored), reports
  an unknown runner as a clean error instead of a traceback, and `--print-baseline` prints the rows it could not
  measure as `null` with a warning, so a paste-over cannot silently un-gate the Betaflight rows (a `null` baseline
  row is no baseline).
- **The indexed MCAP reader cross-checks its message indexes** (record opcode and channel, each entry's time against
  its message header, truncation) and falls back to the mcap iterator on any disagreement: a corrupted index can no
  longer return wrong data silently, and index-only damage no longer fails a read the iterator serves.
- `OFS_BENCH_ACCEPT_SLOWER`'s docs say to delete the repository variable once the baseline PR has merged.

### B2: stabilized-session semantics
#### Changed
- **Repro bundles say format 2.** The layout is unchanged, but a 0.2.0-or-older build now refuses the bundles this
  build writes ("this build reads 1") instead of misreading them — for a stabilized session it would have replayed
  the bundle throttle-only and reported "not reproduced"; this build still reads the format-1 bundles 0.2.0 wrote. The bundle also records what the session actually flew
  (a quad file that is `open_loop_stabilized` records `stabilize = true` even when the request sent no flags).
- Loading a quad file whose kind is `open_loop_stabilized` with `open_loop_fc = true, stabilize = false` is refused
  (invalid argument) instead of silently flying it throttle-only.

### M9b: native Windows SITL (experimental)
#### Added
- **Betaflight SITL without WSL (#67).** `scripts/build-sitl-windows.sh`, run from an MSYS2 shell, builds the pinned
  SITL as a Windows program (`third_party/betaflight/ofs-sitl-windows.patch` on top of `ofs-sitl.patch`) and copies
  its two Cygwin runtime DLLs beside it; set `OFS_SITL_LAUNCH` to the `.exe`. Run from Windows it passed the `ofs-fc`
  and `ofs-video` live tests, the `ofs-sim` live tests that run on Windows (9 of 9; 5 more are Linux-only) and the 26
  Python SITL tests, and measured 2.1 to 2.5 times faster than SITL in WSL 2 in one benchmark run. WSL stays the
  supported path: no CI job builds the native one yet. `docs/research/sitl-native-windows.md` has the findings, what is left, and the sizing of a
  DLL-free port.
- The default stray-SITL cleanup on a plain Windows host is scoped to the session's slot (a PowerShell command; it
  returned nothing before), and a SITL that cannot load its runtime DLLs now says so.
#### Changed
- Two Python test helpers (`msp_status.py`, `test_sitl_telemetry.py`) keep pumping simulated time while they wait for an
  MSP reply instead of blocking on the socket, and `test_per_motor_erpm_reaches_betaflight` samples again until the
  four rpm values differ (a single sample failed 5 runs in 8 against the native SITL).
- The cleanup commands of a SITL launch are given up on after 10 s (`CLEANUP_TIMEOUT`) instead of waited for without a
  bound.

### M9a: parallel SITL sessions
#### Added
- **Several Betaflight sessions on one machine (#15).** Each simulator process takes the first free *slot*, a block of
  ten ports: the first session has the stock ports and the Configurator address `tcp://127.0.0.1:5761`, the next
  `5771` (UDP 9011 to 9014), and so on up to 16. The patched SITL takes `--port-offset N` (0 to 1000), the simulator
  claims a slot by holding sockets the OS releases on any exit, and a session in slot n > 0 uses its own firmware
  directory (`<quad>-<hash>-s<n>`). `LoadReply.configurator_address` says which address a session got.
- `PortMap` and `SitlSlot` in `ofs-fc`, `Vehicle::firmware_dir()` and `slot_firmware_dir` in `ofs-sim`, and the tests
  `python/tests/test_sitl_parallel.py` (two `ofs.launch()` sessions flying Betaflight at once, one closing while the
  other flies) and, in `ofs-sim`, `a_second_session_gets_its_own_slot_and_both_fly`.
#### Changed
- **The stray-SITL cleanup is scoped to the session's slot.** The default cleanup, the wait for its victims and the
  SIGKILL escalation used to match every `betaflight_SITL` on the machine by name, so a second session would have killed
  the first; they now match only processes named `betaflight_SITL` launched with the slot's `--port-offset` (slot 0
  also takes one launched without the option, as 0.2.0 did). A `--port-offset` in `OFS_SITL_LAUNCH` or `fc.launch` is
  replaced by the slot's. A launch argv without the option (a hand-made `OFS_SITL_LAUNCH` in a test) and an explicit
  `OFS_SITL_CLEANUP` keep the by-name behaviour.
- A replay (`ofs-sim --replay`) seeds the bundle's firmware into the slot it takes, so it replays exactly while another
  session is running.
- **Rebuild SITL** with `scripts/build-sitl.sh`: the simulator always passes `--port-offset`, so an older SITL binary
  stops at startup with `[SITL] Unknown argument: --port-offset`.

### A dropped Python `Sim` stops its server (#109)
#### Fixed
- **A `Sim` from `ofs.launch()` that was loaded and then dropped without `close()` is collected, and its server and
  Betaflight stop with it.** The thread reading the event stream held the `Sim`, so it lived (and its server ran, and held
  a SITL slot) until the script exited; a loop that launched a server per round ran out of slots after 16.

## 0.2.0 — 2026-10-17 (phases 2 and 3)
Milestones M5 to M8c. Phase 2 made the models more accurate and the simulator verifiable; phase 3 hardened it, added
race timing and made flights scriptable.
- **A swappable radio (M5):** the radio is three parts (`Handset`, `Link`, `Receiver`) with an ExpressLRS link on 2.4
  GHz or 900 MHz and a geometry-free `fixed` link; a live flight's sticks can be recorded to a `play_inputs` CSV (the
  game's **I** key, `sim.record_inputs()`).
- **More accurate physics and electrics (M6a, M6b):** rotor gyroscopic coupling, windmilling, a soft FC mount, a
  per-cell battery with ageing, a thermal model for the ESCs, motors and VTX, and the video receiver's thresholds as
  world data.
- **Verifiable (M6c):** a failed session writes a repro bundle that `ofs-sim --replay` and `ofs.repro.replay_bundle`
  run again; `ofs-validate` is a harness for community measurements (`docs/validation.md`).
- **Hardened (M6d, M6e, M7a):** stricter config checks, validated and clamped sticks, an inputs recording that always
  finishes, a Blackbox log that is finished even when a session ends armed, a Betaflight reboot that no longer wedges
  when the client hangs up, clearer errors from the recording reader, and two radio fixes (a burst-loss window edge,
  the 2.4 GHz 50 Hz sensitivity).
- **Race timing (M8a):** `[[gates]]` in the world file, split and lap events, a lap clock in the state and on the
  game's HUD (`docs/race-timing.md`).
- **Scripted flight (M8b):** an opt-in angle-mode stabilizer for open-loop sessions, a four-gate circuit
  (`worlds/circuit.toml`) and a pilot script that flies a lap on it and on real Betaflight
  (`docs/scripted-flight.md`).
- **Tooling (M8c):** a CI gate against a recorded benchmark baseline, and a recording reader 4 to 10 times faster.

**Upgrading from 0.1.0**
- The gRPC protocol is version 12 (0.1.0 spoke 9): a 0.1.0 client or server is refused at the handshake, so install the
  Python package and build `ofs-sim` from the same release (the tag `v0.2.0`).
- Rebuild Betaflight SITL with `scripts/build-sitl.sh`: the patch changed (a client that hangs up after `MSP_REBOOT` no
  longer wedges the reboot, and the SITL's ports bind reliably). A custom quad whose Betaflight diff failed silently
  on first boot in 0.1.0 now fails loudly: the diff apply is judged on the config run's exit and its whole output.
- Sticks are validated and clamped for everyone: a non-finite stick is refused, an out-of-range one is clamped to the
  transmitter's range (sticks to [-1, 1], throttle to [0, 1]) before the bus sees it, so the `rc.*` channels of a
  recording show the clamped values. In the Rust API `Vehicle::set_sticks` returns a `Result`, and `ofs-radio` was
  restructured into handset, link and receiver parts (M5): code that builds a link by hand needs porting.
- Flights differ from 0.1.0 wherever the new models act: unbalanced rotors (a yaw input, a failed motor) through
  gyroscopic coupling, any descent through windmilling on the shipped prop, out-of-range sticks (now clamped), and
  seeded radio runs with burst loss or ELRS 2.4 GHz at 50 Hz (a window edge and the sensitivity were fixed).
  Recordings are not byte-identical to 0.1.0's, and the golden digests in the repository were regenerated along the way.
  Do not expect to compare a 0.2.0 run against a stored 0.1.0 one.
- Recordings carry new channels: `prop.{i}.inflow_mps`, `battery.cell.{k}.voltage_v` and `.soc`, the temperatures
  `esc.{i}.temp_c`, `motor.{i}.temp_c` (100 Hz) and `vtx.temp_c`. The pattern `battery.*` now matches 16 signals on the
  6S pack instead of 4, and `motor.*` and `esc.*` now include a 100 Hz temperature next to the 242 Hz channels, so a
  `to_dataframe` selection that mixes them has gaps on the union time grid: `.dropna()` a column before plotting it.
  Scripts that count channels or compare recordings byte for byte need a look.
- The shipped `flat` world now has three gates, so a session in it (the README's `world="flat"`) has a race: `race.*`
  channels, `State.race.present`, and `gate_passed` and `lap_completed` events once the quad flies through them. The
  Godot game draws the gates and shows a lap line.
- New opt-in `stabilize=True` on `load(..., open_loop_fc=True)` (a test stand-in for scripted flight, not Betaflight);
  without it open loop is unchanged.
- A Betaflight session that ends armed steps up to 3 more simulated seconds to disarm, so its Blackbox log is finished;
  a running recording sees those steps.
- The loader refuses a prop part with `windmill_j0` between 0 and 0.2 and a battery whose resistance grows more than
  tenfold.
- Every session now keeps a 10 s tail and a journal in memory for the repro bundle (about 15 MB for the shipped quad,
  about 6 % of the open-loop real-time factor).

The detail by milestone follows.

### M8c: a performance gate and a faster reader
#### Added
- **A benchmark regression gate (#61).** CI's `scripts/benchmark.py` step compares every measured row with a baseline
  recorded for its runner class in `scripts/benchmark-baseline.json` (the median of five green runs on `ubuntu-24.04`:
  open loop 47.9× with recording and 131.9× without, Betaflight 10.0× and 11.8×) and fails a row under 60 % of it, on
  top of the old 1× floor. Each row is the best of two runs (`--repeat 2`) so the 4 % noise between CI runs cannot
  trip it; skipped rows and rows without a baseline are never gated. A deliberate slowdown is accepted by updating the
  baseline in the PR or by setting the repository variable `OFS_BENCH_ACCEPT_SLOWER` to a reason (the table then says
  `ACCEPTED: <reason>`). `--print-baseline` prints the measured rows as baseline JSON.
- `scripts/bench_reader.py`: times the reader against the previous implementation on a fresh recording.
#### Changed
- **`ofs.reader.read` is 4 to 10 times faster (#45)**, with the same signature, result and error messages. On a
  one-minute open-loop recording (27.6 MB, 148 signals) three patterns take 0.34 s instead of 3.3 s and every signal
  2.2 s instead of 9.8 s. It cuts only the selected signals' records out of each chunk using the chunk's message
  indexes. Recordings without message indexes or with lz4 chunks (not what the server writes) are read the old way.

### M8b: scripted flight
#### Added
- **An opt-in stabilizer for open-loop sessions (#66).** `load(..., open_loop_fc=True, stabilize=True)` (Rust:
  `FcKind::OpenLoopStabilized`, or `[fc] kind = "open_loop_stabilized"` in a quad file) adds a minimal angle-mode
  controller, so the roll and pitch sticks set the lean (up to 60 degrees), the yaw stick the turn rate (up to 200 degrees
  per second) and a script can fly a path without Betaflight. It is a test stand-in, not Betaflight: it reads the
  simulator's truth and has no arming, rates, I-term or filters, and real tuning needs SITL. `open_loop_fc` alone is
  unchanged (throttle to every motor). `docs/scripted-flight.md`.
- **A scripted circuit (#53).** `worlds/circuit.toml` (four gates on a 40 m square) and `python/examples/circuit.py`, a
  pilot that steers with the sticks from the quad's state and flies a lap through the gates. `python/tests/test_circuit.py`
  asserts the lap on the stabilizer in every CI run; `python/tests/test_sitl_circuit.py` asserts it on real Betaflight in
  the `sitl` job.
#### Changed
- **Breaking (protocol):** the protocol is version 12 (`LoadRequest.stabilize`), so a client of protocol 11 is refused by
  the handshake. `stabilize` without `open_loop_fc` is refused with `invalid_argument`. A repro bundle records
  `stabilize` (a bundle without the key was never stabilized).

### M8a: gates and lap timing
#### Added
- **Race timing (#71).** A world file declares an ordered course with `[[gates]]` (`name`, `center_ned_m`, `heading_deg`,
  `width_m`, `height_m`; at most 64; gate 0 is the start and finish). The simulator counts the gates the quad flies
  through in order, interpolating each crossing inside the tick, and times the splits and laps (`docs/race-timing.md`).
  `worlds/flat.toml` has three gates.
- Events `gate_passed` and `lap_completed` (messages such as `Gate1 (gate 1) split 8.412 s` and `lap 1: 31.204 s (best)`),
  `State.race` (`present`, `running`, `next_gate`, `gate_count`, `laps`, `current_lap_s`, `last_lap_s`, `best_lap_s`),
  `World.gates`, and `race.*` signals in a recording. A world without gates has none of them.
- The Godot client draws each opening as a translucent rectangle labelled START or with its number, shows a lap line on
  the HUD and toasts the events. Python: `ofs.Gate`, `ofs.Race`, `World.gates`, `State.race`.
#### Changed
- **Breaking (protocol):** the protocol is version 11, so a client of protocol 10 is refused by the handshake. The
  Rust and Python clients in this repository speak it.

### M7a: hardening sweep
#### Changed
- **Breaking (Rust API):** `Vehicle::set_sticks` returns a `Result`: a non-finite stick is refused with an `InvalidArgument` error and
  nothing is applied; what is accepted is clamped to the transmitter's range (sticks to [-1, 1], throttle to [0, 1])
  before the bus, an inputs recording and the journal see it (#54). The gRPC `SetSticks` already refused non-finite
  values; out-of-range ones are now clamped on the bus too (the `rc.*` channels of a recording show the clamped values).
- The inputs recorder writes no row for a change that came back to the written row within the same tick, and always
  writes a first row (#54).
- `Load` and `Unload` stop a running inputs recording, so its file is complete, and publish `inputs_recording_stopped`
  (#55).
- A session on Betaflight that ends while the quad is armed disarms it first (the link and the radio faults are
  restored on the bus so the disarm reaches it, then it steps until the motors read disarmed, 3 simulated seconds at
  most), so its Blackbox log is finished; it used to be cut short, notably when the pilot had left first, which is the
  Godot client's quit order. A running MCAP recording sees those steps; the journal and an inputs recording do not
  (#58). A session that is not armed, or has failed, does not pay for it.
- Reading a recording the Python reader cannot use now says why instead of an mcap record error (#59): `unfinished`
  (no footer: a write error such as a full disk, or a killed server), `damaged` (a half-written last chunk) or
  `not an MCAP file`, each pointing at the `recording_stopped` event where relevant. The writer finishes the file
  best-effort after a write error (a test holds that), but the record being written can be left half written.
- Radio: a packet exactly on a burst-loss window edge belongs to the window that starts there, for the ELRS and the
  fixed link alike (#13), and the 2.4 GHz
  50 Hz sensitivity is -115 dBm, ExpressLRS's published figure, instead of -117 (#12). Both change seeded runs and the
  ELRS golden digests (regenerated on both OSes, in their own commits).
#### Fixed
- The patched Betaflight SITL no longer wedges a reboot when the MSP client hangs up right after `MSP_REBOOT` (#57):
  rebuild SITL with `scripts/build-sitl.sh`.
- The SITL port-bind flake (#14) was fixed in the patch on 2026-10-15 (`6ccbaef`, `ac97cad`); no recurrence since.

### M6e: the rest of the M6 minors
#### Changed
- The default recording table records the ESC and motor temperatures (`esc.{i}.temp_c`, `motor.{i}.temp_c`) at 100 Hz,
  the thermal model's own rate, instead of 240 Hz (they only change at 100 Hz). Recordings and the radio-flight
  golden digests change accordingly (both OS files regenerated); the simulation itself does not. A Python selection
  such as `to_dataframe(["esc.*"])` now mixes two rates (242 Hz and 100 Hz), so its columns have gaps on the union time
  grid: `.dropna()` a column before plotting it, as for any mixed-rate selection.
- ESC telemetry and the VTX built without a thermal model (test rigs) read the default 25 °C ambient, not 0 °C.
  `ofs_core::Bus::signal_or` and `ofs_core::consts::DEFAULT_AMBIENT_C` are new.

### M6d: deferred minors
#### Changed
- The loader refuses what it used to accept: a prop part with `windmill_j0` between 0 and 0.2 (0 still turns windmilling
  off; a small value is a runaway rotor), and a battery whose `resistance_growth_per_cycle × cycles` makes the pack more
  than ten times as resistive. A non-finite `[video]` threshold is reported on its own field.
- `ofs-validate`: a `submission.toml` error names its line; `--max-error-pct` now fails (exit 1) when a row never
  settled; the settle test is one part in a million per second as documented (it was four times looser), so a slow
  rotor settles a little later and closer to its steady state. Without the flag the exit code is unchanged.
- A repro bundle's `journal.txt` must open with its `# ofs-journal 1` header (a byte-order mark is accepted), and
  `ofs.repro.find_bundles` skips a folder that has no `bundle.toml` yet (still being written). `replay_bundle` reads
  `ofs-sim`'s output as UTF-8, so accented paths and Windows error texts come back readable.
#### Fixed
- Stale comments and docs: the cell voltages are the battery model's own, not equal shares.

### M6c: validation and the repro bundle
#### Added
- A failed session writes a **repro bundle**: a folder under the logs directory (`<quad>-<n>.repro`) with the quad,
  the library and world it used, the seed, a journal of everything that steered the run, the firmware's state from
  `Load` and the last 10 seconds of its channels. `ofs-sim --replay <folder>` and `ofs.repro.replay_bundle` run it again
  and say whether the failure came back (`docs/repro-bundle.md`). Every session now keeps that 10 s tail in memory (about 15 MB
  for the shipped quad; it costs about 6 % of the open-loop real-time factor, 275x to 257x) and a journal of its
  steering calls.
- `ofs-validate`, the validation harness: a documented submission format for community measurements (`thrust_stand`,
  and `blackbox_step` as a checked format), a checker that reports every problem with its file and line, and a
  thrust-stand comparator that runs the simulator's own ESC, motor and propeller to steady state at each measured row.
  A synthetic worked example and its generated report are under `validation/`; CI prints the report in the job summary
  (tracked, not gating). See `docs/validation.md`.
- `ofs.repro` (`find_bundles`, `replay_bundle`), and `ofs_config::load_with_library_env`.
#### Changed
- The `sim_error` event's text now ends with `(repro bundle: <folder>)` (or why there is none).
- `ofs-sim --data-dir` has no default shown in `--help` any more (it is still `.ofs-data`); with `--replay` it defaults
  to a temporary directory.

### M6b: battery, thermal and receiver
#### Added
- The video receiver's thresholds are world data: an optional `[video]` table in a world file sets the SNR
  thresholds, field counts and drop probability (`docs/research/video-link.md`). Without it nothing changes.
- A per-cell battery: the pack is a series string of cells with their own resistance, state of charge and capacity;
  battery parts may set a per-cell spread and ageing (`cycles`, capacity fade, resistance growth), the library has an
  aged example pack, and recordings gain `battery.cell.{k}.voltage_v` and `.soc` channels
  (`docs/research/battery-and-thermal.md`).
- A thermal model: one RC node per ESC, motor and VTX, heated by the losses already on the bus, with the ambient
  set by an optional `[environment] ambient_c` in the world file. Parts carry optional `thermal` sections (the library
  parts ship estimated constants), and recordings gain `esc.{i}.temp_c`, `motor.{i}.temp_c` and `vtx.temp_c`
  (`docs/research/battery-and-thermal.md`). Nothing derates.
#### Changed
- The weak-cell fault lands on the cell it names, and the state stream's cell voltages are the battery model's own
  (they used to be equal shares of the pack voltage).
- The recording pattern `battery.*` now also matches the per-cell channels (16 signals on the 6S pack instead of 4); a
  script that counted them, or that relied on `battery.voltage`'s near-miss suggestions, needs a narrower pattern.
- The KISS telemetry frames carry each ESC's simulated temperature and Tramp's status reply the VTX's, instead of a
  constant 25 °C (which is what they still report with no heating, at the default ambient).
- The radio goldens' digests were regenerated for the new recorded channels.

### M6a: rotor and frame physics
#### Added
- Rotor gyroscopic coupling: the spinning rotors' angular momentum now pushes back on a turning airframe
  (`docs/research/rotor-dynamics.md`).
- Windmilling: a propeller's torque now depends on the air coming up through its disc, so an unpowered rotor in a fast
  descent keeps turning. The prop part's new optional `windmill_j0` sets it (0 = off), and recordings gain the
  `prop.{i}.inflow_mps` channel.
- A flight-controller mount: an optional `[frame.fc_mount]` (`natural_hz`, `damping_ratio`) in a frame part passes the
  motors' vibration through a second-order transmissibility before the IMU feels it, and the library part
  `opendrone-5f-softmount` is a soft-mounted example. A frame without it is rigid, as before.
#### Changed
- Flights where the clockwise and counter-clockwise rotors do not stay balanced (a yaw input, a failed or damaged
  motor) differ from 0.1.0; balanced flights, such as the radio goldens' roll and pitch steps, are bit-identical.
- Recordings carry one more channel per motor (`prop.{i}.inflow_mps`), so the radio goldens' digests were regenerated.
  A descent now lowers a prop's torque by the advance-ratio correction (`prop.windmill_j0` on the shipped prop).

### M5: swappable radio components
M5 was specified as a real radio over USB serial; the capture found no CRSF stream on the TX15's port
(`docs/research/serial-radio.md`), so the joystick stays the input and the milestone became the radio's structure and
recording a live flight.
#### Added
- The radio as three swappable parts, `Handset`, `Link` and `Receiver`, stepped by `RadioChain`
  (`crates/ofs-radio`): the `joystick` handset, the `elrs` and `fixed` links and the `crsf` receiver.
- ExpressLRS on 900 MHz: a `band` key (`"2g4"`, the default, or `"900"`) on the `elrs` link, and the quad
  `quads/opendrone-5f-900mhz.toml`. The 900 MHz band's carrier, bandwidth, noise figure and downlink power are
  estimates.
- The `fixed` link (a geometry-free loss fraction, bursts and latency) and its `[radio.fixed]` table, with the quad
  `quads/opendrone-5f-fixed-link.toml`.
- `[radio]` keys `handset` and `receiver` (each defaults to the only variant it has), and a test that swaps a link
  defined inside a test file into a flying chain.
- Recording a live flight's sticks to a `play_inputs` CSV: protocol 10 (`StartInputsRecording`,
  `StopInputsRecording` returning `{path, rows, duration_s}`, the events `inputs_recording_started` and
  `inputs_recording_stopped`), Python `sim.record_inputs()` and `sim.stop_inputs_recording()`, and the game's **I**
  key with a `REC STICKS` indicator. Two lockstep replays of one recording write byte-identical MCAP.
- Golden tests that pin the ELRS link's bytes and a recorded flight's digests, one digest file per OS (the libm
  differs between Windows and Linux).
- `docs/writing-a-radio-model.md` and `docs/research/serial-radio.md`.
#### Changed
- `link_stats_interval_packets` moved from the link to the receiver internally; quad files are unchanged.
- The protocol is version 10: a client built before M5 is refused at the handshake.

### the v1 close-out
#### Fixed
- The first boot's diff apply is judged on the config run's exit and its whole output: a run that failed no longer
  passes because SITL had created `eeprom.bin`, and a good run is no longer refused when its last line was still in
  the pipe.
- A SITL that cannot bind its ports now says which port failed and who held it at the moment of the bind.
- A DVR start on an existing file fails instead of truncating the earlier recording.
- The DVR keys its frames on an integer field index, so a long flight can no longer stretch a recording by a frame;
  the video field rate is defined once (`ofs-core`).
- The goggles' latency is reported without a transmitting VTX.
- The burst-loss validation message states its units (seconds).
- The pre-launch SITL cleanup waits for its victims to exit before the next launch binds (the port-bind flake
  caught by CI); under WSL the wait runs in the VM, and a victim that ignores SIGTERM is SIGKILLed after a grace.
#### Added
- `scripts/benchmark.py`: the real-time factor of headless runs, in CI's job summary (fails below 1×).
- Tests: the ELRS link test runs in CI; a PID saved over MSP persists through a reboot and a new session; a real
  tap overflow counts Betaflight's lost bytes; same-seed lockstep runs with latency and field drops write identical
  recordings; the DVR end to end in the Godot e2e and its JPEG encode; the radio fault cannot move the model's own
  random stream; MSP replies split across reads; the 50 km test reads Betaflight's own RX-loss flag.
- `docs/roadmap.md`, `docs/architecture.md`, `CONTRIBUTING.md`, this changelog.
#### Changed
- The per-milestone carried-debt files moved to `docs/superpowers/history/`; the roadmap replaces them.
- The OSD font atlas's GPL-3.0-only licence is accepted and documented (README, Licence).

## 0.1.0 — 2026-10-13 (v1)
The first release: milestones M0–M4 of `docs/superpowers/specs/2026-10-04-open-fpv-sim-design.md`.
- **Headless core (M0, M1):** real Betaflight SITL in exact lockstep with 6-DOF physics, battery, ESC and motor,
  IMU and barometer models; bit-identical runs with the same seed.
- **Radio and pilot (M2):** the ExpressLRS/CRSF link with failsafe, real-time pacing, the firmware lifecycle, the
  Betaflight Configurator over TCP, and the Godot pilot client with joystick input and an FPV camera.
- **Video (M3):** Betaflight's own OSD over MSP DisplayPort; SmartAudio and Tramp VTX control; a 5.8 GHz analog
  link model that breaks the picture up with distance, attitude, buildings and interference; solid world objects;
  a geometric ELRS link; the FC's own telemetry home; video latency, field drops and a DVR.
- **Sandbox (M4):** a component library with recorded provenance; the v1 fault catalog (motor, prop, battery cell,
  radio loss and bursts, gyro, VTX interference), injectable at a simulated time; MCAP recording (byte-identical
  across identical lockstep runs), pandas/CSV/Parquet export, scripted stick inputs, Betaflight's Blackbox logs, a
  live Rerun view; the `open-fpv-sim` package on PyPI.
