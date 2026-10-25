# Game Content First Batch (issues #130 + #131) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the placeholder-box drone with a modelled procedural quad (#130) and add two new worlds to fly — a tight proximity park and a fast open meadow (#131) — as the first batch of epic #136 (Game: drones, worlds, races and graphics).

**Architecture:** #130 is a rewrite of the static body built in `godot/drone/drone.gd`'s `_ready()`; the public surface (`set_motors(commands, delta)`, the `Prop1..4` pivots, `_props`) is unchanged, so `godot/scripts/app.gd` and the existing prop test are untouched. #131 is pure data: two world TOML files validated by the existing `ofs-config` schema, covered by two new tests in `crates/ofs-config/tests/world.rs` in the shape of `the_circuit_world_loads`. No protocol change, no server change, no new GDScript files.

**Tech Stack:** GDScript (Godot 4.7, tab-indented, headless-tested with the dummy renderer), TOML world files, Rust tests in `ofs-config`.

**Spec:** GitHub issues [#130](https://github.com/hugowis/open-fpv-sim/issues/130) and [#131](https://github.com/hugowis/open-fpv-sim/issues/131) under epic [#136](https://github.com/hugowis/open-fpv-sim/issues/136) (no separate spec doc; their Done-when lists are carried below and argued from here). Branch: `worktree-game-content` in the worktree `C:\dev\open-fpv-sim\.claude\worktrees\game-content` (created from `origin/main` 07dffcb; baselines verified there: `cargo test -p ofs-config` green, Godot unit suite 426 checks / 0 failed).

## Global Constraints

- **Isolation:** all work happens in the worktree above, on branch `worktree-game-content`. The `dvr-bundle` session works in the main checkout on different files (`settings.gd`, `controls_menu.gd`, `dvr.gd`, `test_settings.gd`, `test_controls_menu.gd`, `e2e_open_loop.gd`); nothing here touches those files.
- **No protocol or server change:** telemetry keeps carrying `motor_cmd` (commands, not measured eRPMs — `crates/ofs-client/src/model.rs:423-425`); worlds are schema 1, which `ofs-config` already validates.
- **The default flight path is untouched:** the game still boots `quads/opendrone-5f-freestyle.toml` in `worlds/flat.toml`; `worlds/flat.toml` and `worlds/circuit.toml` are not edited (their recordings, goldens and tests must stay byte-stable).
- **GDScript house style:** tabs for indentation, snake_case, doc comments with `##`; no bare `rustfmt`/`cargo fmt --all` on this repo (rustfmt 1.10.0 disagrees with the committed style on ~153 files) — hand-indent only what you touch.
- **Rust gate:** after any task that lands Rust, `cargo clippy --workspace --all-targets --locked -- -D warnings` must pass (CI's first step; local `cargo test` does not run it).
- **Godot binary:** `GODOT_BIN=/c/tools/godot/Godot_v4.7.2-stable_win64.exe scripts/run-godot-tests.sh unit` (the script auto-builds `ofs-sim` + `ofs-godot` first; a fresh worktree builds them from scratch once).
- **Commits:** explicit `git add <files>` only (never `git add -A`); one commit per task, message style `feat(game): ...` / `docs(game): ...`.
- **Windows traps (do not misdiagnose):** firewall Block rules on fresh exes only affect live-SITL tests (none here); no Python tests in this plan, so no `PYTHONPATH` setup.

## Rulings (deviations from the issue text, binding)

1. **#130 "a real model (glTF)" → procedural low-poly built in `drone.gd`.** A glTF asset needs the Godot editor import step, which the headless dummy-renderer test suite does not run, and adds a licensing/provenance question for a binary asset. The procedural build is in-repo code, testable headless, and a later glTF is a drop-in replacement of `_ready()`. The issue's "real model" bar is met by: recognisable quad-X airframe, camera at the configured tilt, antennas, battery with strap, motor bells, spinning props.
2. **#130 "props spin at each motor's own speed from the state stream" → props spin from motor commands, as today.** The client's telemetry carries `motor_cmd`, not per-motor omegas (`model.rs:423-425`); measured eRPMs would need a protocol addition (out of scope here). A failed or windmilling motor already shows — its command drops and its prop visibly stops. Feeding real eRPMs is a candidate follow-up issue, not part of this plan.
3. **#130 "shown in the menu and the workbench preview" → deferred to #123/#128.** Neither screen exists yet; `drone.gd` (a `Node3D` that builds itself and takes `set_motors`) is exactly the reusable unit they will instantiate.
4. **#131 "loads from the menu" → worlds load via the existing setting** (`OFS_WORLD` / `--world=`, resolved by `world::resolve(name, quad_path)` beside `quads/`); the menu that lists them is #123. This plan proves loadability with the same tier of test the shipped worlds have.
5. **#131 "Godot dressing (materials, colors, props)" → dressing is the world files' own data.** `godot/world/world.gd` already draws every object's position, size and color from the file; the new worlds get their identity from their geometry and palettes, not from renderer changes.

## Review Focus

The input classes most likely to bite, most likely first, each pinned by a test in its owning task:

1. **A tilted gate whose corner dips below the ground** — validation rejects it (`gates[N].center_ned_m: the gate's opening must not reach below the ground`), so a world with a hand-placed rolled/pitched gate fails only at load. The tilt grows the vertical extent to `height·cos(t) + width·sin(t)`; both new worlds' tilted-gate centres were derived with it. Pinned by Tasks 2–3 (the load tests pass only if the maths was right) and the existing `a_gate_tilt_is_optional_and_checked` staying green.
2. **The FPV camera seeing its own nose** — the camera housing, if outside the camera's 0.02 m near-clip sphere, renders as slivers at the frame edge. Pinned by Task 1's near-clip assertion (housing wholly inside the sphere) and the plate-nose assertion.
3. **A dead motor must look dead** — #130's failure-visibility claim rests on a 0-command motor not turning its prop. Pinned by the existing `test_the_drone_has_four_propellers_that_follow_the_motor_commands` (an idle motor does not turn its propeller), kept untouched and green.
4. **The default path regressing** — the game boots `flat`, and every existing world assertion keys on it. Pinned by `the_shipped_world_loads` / `the_circuit_world_loads` staying green and Task 4's open-loop e2e run.
5. **Brittle object-count assertions** — a later edit to a world TOML would fail a bare `len() == N` with no hint. The count asserts in Tasks 2–3 carry a breakdown message (e.g. "gate frames (9), walls (2), pillars (2), hedge") and named-object lookups carry the real weight.

## File Structure

- Modify: `godot/drone/drone.gd` — the modelled quad (Task 1). Public surface unchanged: `set_motors(commands: PackedFloat32Array, delta: float)`, `_props: Array[Node3D]` (pivots `Prop1..4`).
- Modify: `godot/tests/test_scene.gd` — one new test method (Task 1).
- Create: `worlds/park.toml` — proximity park world (Task 2).
- Create: `worlds/meadow.toml` — open meadow world (Task 3).
- Modify: `crates/ofs-config/tests/world.rs` — two new test methods (Tasks 2–3).
- Modify: `docs/race-timing.md`, `README.md` — name the new worlds (Task 4).

World-file field reference (from `crates/ofs-config/src/world.rs` and `worlds/flat.toml`, schema 1): `[pilot] position_ned_m` (must not be below ground) + `facing_deg`; `[receiver] noise_floor_dbm`, `diversity`, `[[receiver.antennas]]` (≥1; unique `name` of letters/digits/`_`/`-`; a `patch` needs `beamwidth_deg`); `[[emitters]]` needs exactly one of `band`+`channel` or `freq_mhz` (5300..=6000), plus `power_mw` and `position_ned_m`; `[[objects]]` with `shape = "box"` (`size_m`) or `"cylinder"` (`radius_m`, `height_m`), `center_ned_m`, `color = [r,g,b]` in [0,1], optional `rf_loss_db` ≥ 0; `[[gates]]` with `name` (unique), `center_ned_m` (lowest edge not below ground), `heading_deg`, `width_m` > 0, `height_m` > 0, optional `pitch_deg` (|pitch| < 90) and `roll_deg` (< 180) — stored as plain `f64`, default 0.0; optional `[race] laps` (u32 ≥ 1, needs ≥1 gate).

---

### Task 1: The modelled quad (#130)

**Files:**
- Modify: `godot/drone/drone.gd` (rewrite the body built by `_ready()`; keep everything else)
- Test: `godot/tests/test_scene.gd` (add one method; do not touch the existing ones)

**Interfaces:**
- Consumes: nothing new. `set_motors(commands: PackedFloat32Array, delta: float)` keeps its contract (positive rotation about +Y is counter-clockwise seen from above; `SPIN` per motor), and `_props` stays an `Array[Node3D]` of four pivots named `Prop1..4`.
- Produces: node names later tasks/screens may rely on: `Plate`, `PlateTop`, `Arm` (×2), `Battery`, `BatteryStrap`, `Motor1..4`, `Prop1..4` (children `Blade`, `Disc`), `Antenna1`, `Antenna2`, `CameraHousing` (child `Lens`). `app.gd`'s call `drone.set_motors(telemetry["motor_cmd"], delta)` is unchanged.

Geometry rulings baked into the numbers below (the near-clip maths, verified):

- The FPV camera (`godot/drone/fpv_camera.gd`) sits at `(0, 0.02, -0.04)` looking −Z with `near = 0.02`: nothing within 0.02 m of that point renders in FPV.
- `CameraHousing` is centred **at the eye point** `(0, 0.02, -0.04)`, size `(0.02, 0.02, 0.026)`: its farthest corner from the centre is `√(0.01² + 0.01² + 0.013²) = 0.0192 < 0.02`, so the whole housing is inside the near-clip sphere — invisible in FPV, fully visible in the chase view (this is the modelled camera at its configured tilt, per #130). Its `Lens` child's farthest corner reaches `0.0197 m`, also inside.
- `Plate` is 0.09 deep centred at `z = 0.015`: its nose reaches `z = -0.03`, behind the camera plane at `-0.04`, so no plate sliver shows in FPV. (The arms and outer props do reach into the FPV frame edge — authentic; a real pilot sees arms and props at the frame edge.)
- Battery, strap, top plate, antennas stay behind `z = -0.04`.

- [ ] **Step 1: Write the failing test**

Add to `godot/tests/test_scene.gd`, after `test_the_drone_has_four_propellers_that_follow_the_motor_commands`:

```gdscript
func test_the_drone_is_a_modelled_quad_the_fpv_camera_never_sees() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	for i in [1, 2, 3, 4]:
		ok(drone.get_node_or_null("Motor%d" % i) != null, "motor bell %d" % i)
		ok(drone.get_node_or_null("Prop%d" % i) != null, "prop %d" % i)
	ok(drone.get_node_or_null("Antenna1") != null and drone.get_node_or_null("Antenna2") != null, "two VTX antennas")
	var eye := Vector3(0.0, 0.02, -0.04)  # fpv_camera.gd's _ready position, near = 0.02
	var housing: MeshInstance3D = drone.get_node("CameraHousing")
	eq(housing.position, eye, "the camera housing sits at the FPV camera's eye point")
	var corner: float = ((housing.mesh as BoxMesh).size * 0.5).length()
	ok(corner < 0.02, "the housing hides inside the near clip (corner %f)" % corner)
	var plate: MeshInstance3D = drone.get_node("Plate")
	var plate_nose: float = plate.position.z - (plate.mesh as BoxMesh).size.z * 0.5
	ok(plate_nose >= eye.z, "the plate's nose stays behind the FPV camera")
	var battery_material: StandardMaterial3D = drone.get_node("Battery").material_override
	ok(battery_material.albedo_color.is_equal_approx(Color(0.9, 0.55, 0.1)), "the battery stays orange")
	ok((drone.get_node("Plate").material_override as StandardMaterial3D).albedo_color.r < 0.2, "the plate is carbon")
	drone.queue_free()
```

- [ ] **Step 2: Run the suite, verify the new test fails**

Run: `GODOT_BIN=/c/tools/godot/Godot_v4.7.2-stable_win64.exe scripts/run-godot-tests.sh unit`
Expected: FAIL in `test_the_drone_is_a_modelled_quad_the_fpv_camera_never_sees` — `CameraHousing` (and the motors/antennas) do not exist yet; every other test stays green (426 checks before, the same failures only in the new test).

- [ ] **Step 3: Rewrite the body in `godot/drone/drone.gd`**

Replace the header constants and `_ready()` with the code below. Keep the file's existing doc comment (update its first line to say the drone is modelled, not "a small quad drawn at the pose…"), and keep `set_motors`, `_material`, `_add_box` exactly as they are. Tab indentation.

```gdscript
const ARM := 0.08
const MOTORS := [Vector3(ARM, 0, ARM), Vector3(ARM, 0, -ARM), Vector3(-ARM, 0, ARM), Vector3(-ARM, 0, -ARM)]
## +1: the propeller turns clockwise seen from above (Betaflight's default direction for each motor).
const SPIN := [1.0, -1.0, -1.0, 1.0]
## A real propeller turns far too fast to show; this is the drawn speed at a full motor command, in rad/s.
const DRAWN_SPIN := 60.0

const CARBON := Color(0.10, 0.10, 0.11)
const ARM_COLOR := Color(0.2, 0.2, 0.22)
const BATTERY_COLOR := Color(0.9, 0.55, 0.1)
const STRAP_COLOR := Color(0.15, 0.15, 0.16)
const BELL_COLOR := Color(0.16, 0.16, 0.18)
const PROP_COLOR := Color(0.08, 0.08, 0.08)
const ANTENNA_COLORS := [Color(0.85, 0.2, 0.2), Color(0.2, 0.4, 0.9)]
const HOUSING_COLOR := Color(0.08, 0.08, 0.09)
const LENS_COLOR := Color(0.02, 0.02, 0.03)
## The FPV camera's eye point (fpv_camera.gd): the modelled camera housing is centred there, wholly inside the
## camera's 0.02 m near clip, so the FPV view never sees its own nose. The chase camera sees the full model.
const FPV_EYE := Vector3(0.0, 0.02, -0.04)
## The housing's tilt matches the FPV camera's default up-tilt.
const CAM_UPTILT_DEG := 30.0

var _props: Array[Node3D] = []


func _ready() -> void:
	_add_box("Plate", Vector3(0, 0, 0.015), Vector3(0.034, 0.005, 0.09), CARBON)
	_add_box("PlateTop", Vector3(0, 0.028, 0.012), Vector3(0.034, 0.004, 0.085), CARBON)
	for sign_x in [-1.0, 1.0]:
		var arm := _add_box("Arm", Vector3.ZERO, Vector3(0.24, 0.008, 0.014), ARM_COLOR)
		arm.rotation_degrees.y = 45.0 * sign_x
	_add_box("Battery", Vector3(0, 0.0425, 0.01), Vector3(0.035, 0.025, 0.09), BATTERY_COLOR)
	_add_box("BatteryStrap", Vector3(0, 0.0425, 0.01), Vector3(0.041, 0.028, 0.012), STRAP_COLOR)
	for i in MOTORS.size():
		_add_box("Motor%d" % (i + 1), MOTORS[i] + Vector3(0, 0.016, 0), Vector3(0.052, 0.024, 0.052), BELL_COLOR)
		var pivot := Node3D.new()
		pivot.name = "Prop%d" % (i + 1)
		pivot.position = MOTORS[i] + Vector3(0, 0.028, 0)
		add_child(pivot)
		_props.append(pivot)
		var blade := MeshInstance3D.new()
		var blade_mesh := BoxMesh.new()
		blade_mesh.size = Vector3(0.127, 0.002, 0.012)
		blade.mesh = blade_mesh
		blade.material_override = _material(PROP_COLOR)
		pivot.add_child(blade)
		var disc := MeshInstance3D.new()
		var disc_mesh := CylinderMesh.new()
		disc_mesh.top_radius = 0.0635
		disc_mesh.bottom_radius = 0.0635
		disc_mesh.height = 0.001
		disc.mesh = disc_mesh
		var disc_material := _material(Color(1, 1, 1, 0.12))
		disc_material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		disc.material_override = disc_material
		pivot.add_child(disc)
	for i in 2:
		var antenna := _add_box("Antenna%d" % (i + 1), Vector3(0.014 if i == 1 else -0.014, 0.058, 0.05), Vector3(0.007, 0.075, 0.007), ANTENNA_COLORS[i])
		antenna.rotation_degrees.x = 40.0  # the VTX antenna leans back, as the quad's mount_frd asks
	var housing := _add_box("CameraHousing", FPV_EYE, Vector3(0.02, 0.02, 0.026), HOUSING_COLOR)
	housing.rotation_degrees.x = CAM_UPTILT_DEG
	var lens := MeshInstance3D.new()
	var lens_mesh := BoxMesh.new()
	lens_mesh.size = Vector3(0.014, 0.014, 0.004)
	lens.name = "Lens"
	lens.mesh = lens_mesh
	var lens_material: StandardMaterial3D = _material(LENS_COLOR)
	lens_material.roughness = 0.2
	lens.material_override = lens_material
	lens.position = Vector3(0, 0, -0.015)
	housing.add_child(lens)
```

(The old `Body` grey box goes away; the battery keeps its size and colour but rides on the top plate now (its bottom at 0.030, the plate's top); the motor bells sit on the arms (bottom at 0.004, the arm's top); the prop pivots rise from 0.012 to 0.028, onto the bells; the antennas' bases land on the top plate at 0.030.)

- [ ] **Step 4: Run the suite, verify it passes**

Run: `GODOT_BIN=/c/tools/godot/Godot_v4.7.2-stable_win64.exe scripts/run-godot-tests.sh unit`
Expected: PASS — the new test green, `test_the_drone_has_four_propellers_that_follow_the_motor_commands` still green (proof the `set_motors` surface is intact), all other suites unchanged (0 failed; the check count grows by the new test's checks).

- [ ] **Step 5: Look at it once by hand (the numbers can pass and the model still look wrong)**

Run: `cd /c/dev/open-fpv-sim/.claude/worktrees/game-content && /c/tools/godot/Godot_v4.7.2-stable_win64.exe --path . -- --open-loop` (press **C** for the chase view; the build from the test steps has the server and extension ready).
Expected: a recognisable dark quad-X with an orange battery, two antennas leaning back, four motor bells with translucent prop discs, and a small tilted camera box at the nose; **C** toggles to it; the FPV view shows no housing slivers. No code changes expected unless something is visually broken (e.g. a part floating off the frame).

- [ ] **Step 6: Commit**

```bash
git add godot/drone/drone.gd godot/tests/test_scene.gd
git commit -m "feat(game): a modelled quad replaces the placeholder boxes (#130)"
```

---

### Task 2: The proximity park world (#131)

**Files:**
- Create: `worlds/park.toml`
- Test: `crates/ofs-config/tests/world.rs` (add one method after `the_circuit_world_loads`)

**Interfaces:**
- Consumes: the schema-1 world loader (`ofs_config::world::load`) and its validation, unchanged.
- Produces: the world name `"park"` resolving through `world::resolve("park", <quad path>)` → `worlds/park.toml` (the existing resolution rule, no code). The Godot client draws it with zero changes; it flies today via `OFS_WORLD=park` / `--world=park`.

Course design (all numbers final; the tilted-gate maths is shown so the reviewer can replay it):
- A diamond loop flown counter-clockwise seen from above: Gate0 (start/finish) `[12, 0]` flown **east** (heading 90), Gate1 `[20, 9]` **north** (0), Gate2 `[28, 0]` **west** (270, **rolled 15°**), Gate3 `[20, -9]` **south** (180), back to Gate0. Legs ≈ 12–14 m. Openings 2.4 m × 2.2 m, upright gates centred at `z = -1.1` (opening spans 0..−2.2, its lowest edge exactly on the ground — the loader accepts exactly-touching, see `a_gate_tilt_is_optional_and_checked`).
- Gate2's roll grows its vertical extent to `2.2·cos15° + 2.4·sin15° = 2.746 m`, half 1.373; centred at `z = -1.5` its lowest edge is `-0.127` — above ground, so it loads (the validator's own quarter-turn test confirms the `height·cos + width·sin` formula). A rolled frame cannot be drawn with axis-aligned boxes (#28), so Gate2 has no posts and bar.
- `[race] laps = 5`.
- Gate0's physical frame flies east, so its posts separate **north–south along x**: posts at `[12 ± 1.25, 0, -1.1]` (inner faces ±1.2 → the 2.4 m opening), bar `[12, 0, -2.25]` spanning 2.6 m of x. Gate1 and Gate3 fly north/south, so their posts separate along y.

- [ ] **Step 1: Write the failing test**

Add to `crates/ofs-config/tests/world.rs` after `the_circuit_world_loads` (the file already imports `Shape`):

```rust
#[test]
fn the_park_world_loads() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/park.toml"))).unwrap();
    assert_eq!(w.name, "Proximity park");
    assert_eq!(w.race.laps, Some(5));
    let gates: Vec<(&str, [f64; 3], f64)> = w.gates.iter().map(|g| (g.name.as_str(), g.center_ned_m, g.heading_deg)).collect();
    assert_eq!(
        gates,
        [
            ("Gate0", [12.0, 0.0, -1.1], 90.0),
            ("Gate1", [20.0, 9.0, -1.1], 0.0),
            ("Gate2", [28.0, 0.0, -1.5], 270.0),
            ("Gate3", [20.0, -9.0, -1.1], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 2.4 && g.height_m == 2.2));
    assert_eq!(w.gates[2].roll_deg, 15.0, "the west gate leans with the turn");
    let names: Vec<&str> = w.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 14, "gate frames (9), walls (2), pillars (2), hedge");
    let post = w.objects.iter().find(|o| o.name == "Gate0PostL").unwrap();
    assert_eq!(post.center_ned_m, [10.75, 0.0, -1.1], "Gate0 flies east: its posts separate along x");
    let hedge = w.objects.iter().find(|o| o.name == "Hedge").unwrap();
    assert_eq!((hedge.shape, hedge.rf_loss_db), (Shape::Box, 6.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!(w.emitters[0].freq_mhz, Some(5806.0));
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p ofs-config the_park_world_loads`
Expected: FAIL — `worlds/park.toml` does not exist (`ConfigError::Io` / unwrap panic).

- [ ] **Step 3: Write `worlds/park.toml`**

```toml
# A proximity park: walls, pillars and a hedge around a tight four-gate diamond, made for slow airframes and
# precise flying (epic #136, issue #131). Gate2 is rolled 15 degrees so its opening leans with the turn; a rolled
# frame cannot be drawn with axis-aligned boxes, so Gate2 has no posts and bar.
#
# Positions are NED metres from home (north, east, down: up is negative); sizes are [north, east, height].
# All values are estimates.
schema_version = 1
name = "Proximity park"

[pilot]
# At the west entrance, goggles 1.7 m up, looking north over the loop.
position_ned_m = [-3.0, 2.0, -1.7]
facing_deg = 0.0

[receiver]
noise_floor_dbm = -93.0
diversity = false

[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"

# A quad left on by the workshop shed, south of the loop, on a channel of its own.
[[emitters]]
name = "workshop-quad"
freq_mhz = 5806.0
power_mw = 25.0
position_ned_m = [18.0, -16.0, -1.0]

# Gate0 is the start and finish, flown east: its posts separate north-south along x.
[[objects]]
name = "Gate0PostL"
shape = "box"
center_ned_m = [10.75, 0.0, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.9, 0.3, 0.2]

[[objects]]
name = "Gate0PostR"
shape = "box"
center_ned_m = [13.25, 0.0, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.9, 0.3, 0.2]

[[objects]]
name = "Gate0Bar"
shape = "box"
center_ned_m = [12.0, 0.0, -2.25]
size_m = [2.6, 0.1, 0.1]
color = [0.9, 0.3, 0.2]

# Gate1, flown north: posts separate east-west along y.
[[objects]]
name = "Gate1PostL"
shape = "box"
center_ned_m = [20.0, 7.75, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.9, 0.45, 0.15]

[[objects]]
name = "Gate1PostR"
shape = "box"
center_ned_m = [20.0, 10.25, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.9, 0.45, 0.15]

[[objects]]
name = "Gate1Bar"
shape = "box"
center_ned_m = [20.0, 9.0, -2.25]
size_m = [0.1, 2.6, 0.1]
color = [0.9, 0.45, 0.15]

# Gate2 (rolled 15 degrees) and its course-side dressing: no physical frame, the opening alone marks it.
[[objects]]
name = "EastWall"
shape = "box"
center_ned_m = [20.0, 13.5, -1.5]
size_m = [10.0, 0.3, 3.0]
color = [0.75, 0.72, 0.65]
rf_loss_db = 12.0

[[objects]]
name = "SouthWall"
shape = "box"
center_ned_m = [8.0, 0.0, -1.5]
size_m = [0.3, 7.0, 3.0]
color = [0.75, 0.72, 0.65]
rf_loss_db = 12.0

[[objects]]
name = "PillarA"
shape = "cylinder"
center_ned_m = [16.0, 4.5, -2.0]
radius_m = 0.3
height_m = 4.0
color = [0.8, 0.35, 0.25]

[[objects]]
name = "PillarB"
shape = "cylinder"
center_ned_m = [24.0, -4.5, -2.0]
radius_m = 0.3
height_m = 4.0
color = [0.8, 0.35, 0.25]

# A low hedge across the middle of the loop: hop it on the Gate0-Gate2 leg.
[[objects]]
name = "Hedge"
shape = "box"
center_ned_m = [20.0, 0.0, -0.4]
size_m = [6.0, 0.8, 0.8]
color = [0.25, 0.5, 0.2]
rf_loss_db = 6.0

# Gate3, flown south: posts along y, like Gate1.
[[objects]]
name = "Gate3PostL"
shape = "box"
center_ned_m = [20.0, -10.25, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.2, 0.6, 0.9]

[[objects]]
name = "Gate3PostR"
shape = "box"
center_ned_m = [20.0, -7.75, -1.1]
size_m = [0.1, 0.1, 2.2]
color = [0.2, 0.6, 0.9]

[[objects]]
name = "Gate3Bar"
shape = "box"
center_ned_m = [20.0, -9.0, -2.25]
size_m = [0.1, 2.6, 0.1]
color = [0.2, 0.6, 0.9]

# The loop: Gate0 east, Gate1 north, Gate2 west (rolled), Gate3 south, back to Gate0.
[[gates]]
name = "Gate0"
center_ned_m = [12.0, 0.0, -1.1]
heading_deg = 90.0
width_m = 2.4
height_m = 2.2

[[gates]]
name = "Gate1"
center_ned_m = [20.0, 9.0, -1.1]
heading_deg = 0.0
width_m = 2.4
height_m = 2.2

[[gates]]
name = "Gate2"
center_ned_m = [28.0, 0.0, -1.5]
heading_deg = 270.0
roll_deg = 15.0
width_m = 2.4
height_m = 2.2

[[gates]]
name = "Gate3"
center_ned_m = [20.0, -9.0, -1.1]
heading_deg = 180.0
width_m = 2.4
height_m = 2.2

[race]
laps = 5
```

- [ ] **Step 4: Run the config tests, verify they pass**

Run: `cargo test -p ofs-config`
Expected: PASS — the new test green, `the_shipped_world_loads` / `the_circuit_world_loads` / the validation tests all still green.

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS (a test-only change; run it anyway — CI does).

```bash
git add worlds/park.toml crates/ofs-config/tests/world.rs
git commit -m "feat(game): the proximity park world (#131)"
```

---

### Task 3: The open meadow world (#131)

**Files:**
- Create: `worlds/meadow.toml`
- Test: `crates/ofs-config/tests/world.rs` (add one method after `the_park_world_loads`)

**Interfaces:**
- Consumes/produces: as Task 2 — world name `"meadow"` → `worlds/meadow.toml`; client unchanged.

Course design (all numbers final):
- A big counter-clockwise loop: Gate0 `[30, 0]` east (90), Gate1 `[60, 45]` **north-east diagonal** (heading 45), Gate2 `[90, 0]` west (270, **pitched 20°**), Gate3 `[60, -45]` south (180), back to Gate0. Legs ≈ 54 m. Openings 8 m × 5 m — a course for the 5" and 7".
- Gate2's pitch grows its vertical extent to `5·cos20° + 8·sin20° = 7.434 m`, half 3.717; centred at `z = -3.8` its lowest edge is `-0.083` — above ground, so it loads.
- Gate1's frame would be diagonal, so it gets posts only (at `±4.075` across the 45° travel: across unit `(−0.7071, 0.7071)`, offset `4.075·0.7071 = 2.8815` → `[57.12, 47.88]` and `[62.88, 42.12]`), no bar. Gate0/Gate3 get full frames (posts at `±4.075`, inner faces ±4.0 = the 8 m opening; bar bottom at `−5.075 + 0.075 = −5.0` = the 5 m opening). Gate2 (pitched) has no frame.
- `[race] laps = 3`. One emitter (the barn's VTX) and scattered trees/barn/bales give the link something to block.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_meadow_world_loads() {
    let w = world::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/meadow.toml"))).unwrap();
    assert_eq!(w.name, "Open meadow");
    assert_eq!(w.race.laps, Some(3));
    let gates: Vec<(&str, [f64; 3], f64)> = w.gates.iter().map(|g| (g.name.as_str(), g.center_ned_m, g.heading_deg)).collect();
    assert_eq!(
        gates,
        [
            ("Gate0", [30.0, 0.0, -2.5], 90.0),
            ("Gate1", [60.0, 45.0, -2.5], 45.0),
            ("Gate2", [90.0, 0.0, -3.8], 270.0),
            ("Gate3", [60.0, -45.0, -2.5], 180.0),
        ]
    );
    assert!(w.gates.iter().all(|g| g.width_m == 8.0 && g.height_m == 5.0));
    assert_eq!(w.gates[2].pitch_deg, 20.0, "the west gate leans up with the climb");
    let names: Vec<&str> = w.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names.len(), 17, "gate frames (8), trees (6), barn, hay bales (2)");
    let post = w.objects.iter().find(|o| o.name == "Gate1PostL").unwrap();
    assert_eq!(post.center_ned_m, [57.12, 47.88, -2.5], "Gate1's posts sit across the 45-degree travel");
    let barn = w.objects.iter().find(|o| o.name == "Barn").unwrap();
    assert_eq!((barn.shape, barn.size_m, barn.rf_loss_db), (Shape::Box, Some([10.0, 6.0, 6.0]), 25.0));
    assert_eq!(w.emitters.len(), 1);
    assert_eq!(w.emitters[0].freq_mhz, Some(5732.0));
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p ofs-config the_meadow_world_loads`
Expected: FAIL — `worlds/meadow.toml` does not exist.

- [ ] **Step 3: Write `worlds/meadow.toml`**

```toml
# An open meadow: a big, fast four-gate loop for the 5 inch and 7 inch quads (epic #136, issue #131). Trees, a
# barn and hay bales give the video link something to block. Gate2 is pitched 20 degrees, leaning its opening up
# on the west leg; a pitched frame cannot be drawn with axis-aligned boxes, so Gate2 has no posts and bar, and
# the diagonal Gate1 gets posts only.
#
# Positions are NED metres from home (north, east, down: up is negative); sizes are [north, east, height].
# All values are estimates.
schema_version = 1
name = "Open meadow"

[pilot]
# Southwest of the course, goggles 1.7 m up, looking north.
position_ned_m = [-5.0, 3.0, -1.7]
facing_deg = 0.0

[receiver]
noise_floor_dbm = -93.0
diversity = true

[[receiver.antennas]]
name = "omni"
kind = "omni"
gain_dbi = 2.0
polarization = "rhcp"

[[receiver.antennas]]
name = "patch"
kind = "patch"
gain_dbi = 8.0
beamwidth_deg = 60.0
polarization = "rhcp"
aim_az_deg = 0.0
aim_el_deg = 10.0

# The barn's VTX, left on, 0.2 m over its roof.
[[emitters]]
name = "barn-vtx"
freq_mhz = 5732.0
power_mw = 25.0
position_ned_m = [40.0, 30.0, -6.2]

# Gate0 is the start and finish, flown east: posts separate along x.
[[objects]]
name = "Gate0PostL"
shape = "box"
center_ned_m = [25.925, 0.0, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate0PostR"
shape = "box"
center_ned_m = [34.075, 0.0, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate0Bar"
shape = "box"
center_ned_m = [30.0, 0.0, -5.075]
size_m = [8.3, 0.15, 0.15]
color = [0.2, 0.8, 0.35]

# Gate1 is the diagonal (heading 45): posts across the travel, no bar (a bar would need a rotated box).
[[objects]]
name = "Gate1PostL"
shape = "box"
center_ned_m = [57.12, 47.88, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate1PostR"
shape = "box"
center_ned_m = [62.88, 42.12, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

# Trees: trunks only at this distance, close enough to matter to the link and the prop spheres.
[[objects]]
name = "Tree1"
shape = "cylinder"
center_ned_m = [20.0, -20.0, -3.0]
radius_m = 0.5
height_m = 6.0
color = [0.2, 0.45, 0.15]

[[objects]]
name = "Tree2"
shape = "cylinder"
center_ned_m = [45.0, 20.0, -3.5]
radius_m = 0.7
height_m = 7.0
color = [0.2, 0.45, 0.15]

[[objects]]
name = "Tree3"
shape = "cylinder"
center_ned_m = [70.0, 15.0, -2.5]
radius_m = 0.4
height_m = 5.0
color = [0.25, 0.5, 0.18]

[[objects]]
name = "Tree4"
shape = "cylinder"
center_ned_m = [80.0, -25.0, -4.0]
radius_m = 0.8
height_m = 8.0
color = [0.2, 0.45, 0.15]

[[objects]]
name = "Tree5"
shape = "cylinder"
center_ned_m = [35.0, -35.0, -3.0]
radius_m = 0.6
height_m = 6.0
color = [0.25, 0.5, 0.18]

# Inside the loop, off the course lines.
[[objects]]
name = "Tree6"
shape = "cylinder"
center_ned_m = [75.0, -8.0, -3.5]
radius_m = 0.5
height_m = 7.0
color = [0.2, 0.45, 0.15]

[[objects]]
name = "Barn"
shape = "box"
center_ned_m = [40.0, 30.0, -3.0]
size_m = [10.0, 6.0, 6.0]
color = [0.7, 0.3, 0.25]
rf_loss_db = 25.0

[[objects]]
name = "HayBale1"
shape = "box"
center_ned_m = [50.0, 10.0, -0.9]
size_m = [1.8, 1.2, 1.8]
color = [0.85, 0.7, 0.3]

[[objects]]
name = "HayBale2"
shape = "box"
center_ned_m = [55.0, -12.0, -0.9]
size_m = [1.8, 1.2, 1.8]
color = [0.85, 0.7, 0.3]

# Gate3, flown south: posts along y, full frame.
[[objects]]
name = "Gate3PostL"
shape = "box"
center_ned_m = [60.0, -49.075, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate3PostR"
shape = "box"
center_ned_m = [60.0, -40.925, -2.5]
size_m = [0.15, 0.15, 5.0]
color = [0.2, 0.8, 0.35]

[[objects]]
name = "Gate3Bar"
shape = "box"
center_ned_m = [60.0, -45.0, -5.075]
size_m = [0.15, 8.3, 0.15]
color = [0.2, 0.8, 0.35]

# The loop: Gate0 east, Gate1 north-east, Gate2 west (pitched up 20 degrees), Gate3 south, back to Gate0.
[[gates]]
name = "Gate0"
center_ned_m = [30.0, 0.0, -2.5]
heading_deg = 90.0
width_m = 8.0
height_m = 5.0

[[gates]]
name = "Gate1"
center_ned_m = [60.0, 45.0, -2.5]
heading_deg = 45.0
width_m = 8.0
height_m = 5.0

[[gates]]
name = "Gate2"
center_ned_m = [90.0, 0.0, -3.8]
heading_deg = 270.0
pitch_deg = 20.0
width_m = 8.0
height_m = 5.0

[[gates]]
name = "Gate3"
center_ned_m = [60.0, -45.0, -2.5]
heading_deg = 180.0
width_m = 8.0
height_m = 5.0

[race]
laps = 3
```

- [ ] **Step 4: Run the config tests, verify they pass**

Run: `cargo test -p ofs-config`
Expected: PASS — both new world tests green, everything else unchanged.

- [ ] **Step 5: Run clippy and commit**

Run: `cargo clippy --workspace --all-targets --locked -- -D warnings`
Expected: PASS.

```bash
git add worlds/meadow.toml crates/ofs-config/tests/world.rs
git commit -m "feat(game): the open meadow world (#131)"
```

---

### Task 4: Docs and the full gate

**Files:**
- Modify: `docs/race-timing.md` (one sentence), `README.md` (one clause)

**Interfaces:**
- Consumes: the two shipped world names and course shapes from Tasks 2–3.
- Produces: documentation that names three shipped worlds instead of one.

- [ ] **Step 1: Update `docs/race-timing.md`**

In the section "The world file", replace:

```
A gate is a *trigger*: it does not collide with the quad and it blocks no signal. Draw its frame with ordinary
`[[objects]]` if you want something to hit: `worlds/flat.toml` has the posts and bar of its three gates as boxes, and
the `[[gates]]` are what those leave free (3.08 m between the posts, 2.44 m under the bar). Godot draws each opening
as a translucent rectangle labelled START or with the gate's number.
```

with (the paragraph, then one added sentence):

```
A gate is a *trigger*: it does not collide with the quad and it blocks no signal. Draw its frame with ordinary
`[[objects]]` if you want something to hit: `worlds/flat.toml` has the posts and bar of its three gates as boxes, and
the `[[gates]]` are what those leave free (3.08 m between the posts, 2.44 m under the bar). Godot draws each opening
as a translucent rectangle labelled START or with the gate's number. The shipped worlds show three scales of course:
`flat`'s straight three-gate line, `park`'s tight walled diamond with one rolled gate, and `meadow`'s big fast loop
with one pitched gate.
```

- [ ] **Step 2: Update `README.md`**

In "The Godot pilot client" section, replace:

```
and the battery reaches Betaflight so its OSD shows
real voltage and warnings. The world (`worlds/flat.toml`: launch pad, gates, buildings, the pilot's spot and a parked
quad's VTX) is a data file the simulator loads; its analog video link degrades the picture and the OSD together, like
real goggles.
```

with:

```
and the battery reaches Betaflight so its OSD shows
real voltage and warnings. The world is a data file the simulator loads — `worlds/flat.toml` (launch pad, gates,
buildings, the pilot's spot and a parked quad's VTX), `worlds/park.toml` (a tight walled proximity loop) and
`worlds/meadow.toml` (a big fast circuit) — and its analog video link degrades the picture and the OSD together, like
real goggles.
```

(The later README sentence "loads `quads/opendrone-5f-freestyle.toml` in `worlds/flat.toml` in real time" stays: the default is unchanged.)

- [ ] **Step 3: Run the whole gate**

Run, from the worktree:
1. `cargo test -p ofs-config`
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`
3. `GODOT_BIN=/c/tools/godot/Godot_v4.7.2-stable_win64.exe scripts/run-godot-tests.sh open-loop` (boots the unchanged `flat` default path end to end; needs `cargo build -p ofs-sim`, which the script runs)

Expected: all three PASS.

- [ ] **Step 4: Commit**

```bash
git add docs/race-timing.md README.md
git commit -m "docs(game): the shipped worlds and courses (#131)"
```

---

## Execution notes

- The worktree is ready with green baselines; a fresh-worktree first build (the Godot script's `cargo build -p ofs-sim -p ofs-godot`) takes a few minutes once.
- Tasks 1 and 2/3 touch disjoint files (GDScript vs TOML+Rust) and can run in either order or in parallel; Task 4 last.
- After the final task, the branch is `worktree-game-content`, 4 commits ahead of `origin/main`; integration (push, PR, or merging into a batch with #129/#132) is a decision for after review — the parallel `dvr-bundle` branch merges first if it lands before this does.
