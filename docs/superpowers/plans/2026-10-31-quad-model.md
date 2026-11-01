# A Real 3D Model for the Quad (#130) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The drone in the chase camera becomes a glTF model (`godot/drone/quad.gltf`) whose propellers turn at each motor's own simulated speed, that scales with the chosen quad's frame and is previewed turning in the main menu — the remainder of #130 after PR #147's in-code model. Closes #130.

**Architecture:** A committed generator (`tools/make_quad_model.py`, the OSD-font precedent) writes the asset from the part files' geometry; `drone.gd` instantiates it at runtime through `GLTFDocument` (no editor import step — the headless suites never import), finds the propeller pivots by the asset's node names, and keeps its `set_motors`/`_props` surface. A new `quad_spec.gd` line-scans the quad TOML and its frame part (the way `catalog.gd` scans world files) for the arm and spin directions; a new `quad_preview.gd` (SubViewport + camera + turntable) is hosted by the main menu and re-hosted by #128's workbench later. No Rust, proto or Python change anywhere: `motor_rpm` has been on the wire since the flight audio (PR #162).

**Tech Stack:** GDScript (Godot 4.7.2, tab-indented, headless-tested with the dummy renderer), Python 3 stdlib (the generator), glTF 2.0.

**Spec:** `docs/superpowers/specs/2026-10-31-quad-model-design.md` — the binding design calls live there; this plan argues from it. The issue is #130 under epic #136 (`gh issue view 130 --repo hugowis/open-fpv-sim`); the phase umbrella is `docs/superpowers/plans/2026-10-31-game-completion.md` (task 1 of 6).

## Global Constraints

- **No Rust, proto or Python-client change**: nothing under `crates/` or `python/` is touched. `motor_rpm` already flows: `crates/ofs-godot/src/lib.rs:274-275` exposes it in `get_telemetry` as a `PackedFloat32Array` (added by PR #162, `d07e0fb`). If a task seems to need a wire change, stop and ledger it instead.
- **The epic rules hold** (#136): presentation in the client only, the models do not change; the analog video chain keeps deciding the goggles' picture — the FPV camera node (`godot/drone/fpv_camera.gd`) is not touched, and no new node renders in FPV (the mount stays inside the 0.02 m near clip at every scale).
- **Godot gates**: `bash scripts/run-godot-tests.sh unit` in WSL after every task (it builds `ofs-sim` + `ofs-godot` and downloads Godot 4.7.2 on first use; on this Windows host `GODOT_BIN=/c/tools/godot/Godot_v4.7.2-stable_win64.exe scripts/run-godot-tests.sh unit` from Git Bash is the equivalent — pick one and stick to it). Before the last commit: `bash scripts/run-godot-tests.sh all` — the e2e suites (including the menu-less `--menu=off` path) must stay green.
- **New test files must be registered**: `godot/tests/run_tests.gd` lists every suite in `SUITES` (lines 6-25); a file not listed never runs.
- **GDScript house style**: tabs for indentation, `snake_case`, `##` doc comments; explicit `git add <files>` only (never `git add -A`); one commit per task.
- **Worktree-isolated sessions refuse heredocs and WSL through Bash**: use Write/Edit for files; for multi-line WSL commands write a script file in the scratchpad and run `wsl -d Ubuntu -- bash <path>`.
- **Commit trailers**: none prescribed — the most recent merged work (B6, PR #167) carries no trailer; an executing session that adds one names its own identity, never a borrowed one.
- **Re-verify every anchor at execution** (the B6 lesson: an evidence line copied on trust was wrong). This plan was written 2026-10-31 against `main` at `87c5914` in the worktree `docs/game-completion-plan`; PR #167 (the B6 sweep) merged as `b37aae5` *after* that base and touched `godot/scripts/app.gd` (a DVR toast string, ~line 385) and `crates/ofs-godot/src/lib.rs` — neither overlaps this plan's edits, but re-check the `set_motors` call site (`app.gd:247-248`) and the `get_telemetry` key list after rebasing.
- **Baselines**: the planning session ran no builds (planning only). The executor's first action is one `unit` run on the untouched tree; ledger any pre-existing failure before starting Task 1.

## Design calls

Decided in the spec (`docs/superpowers/specs/2026-10-31-quad-model-design.md`), one line each; the executor ledgers any change:

1. "A real model" = a glTF 2.0 asset loaded at runtime, replacing the code-built boxes (not sculpted realism).
2. The asset is authored in this repo by a committed generator + provenance note (`godot/drone/QUAD_MODEL.md`); no third-party download, no licence owed.
3. Runtime load via `GLTFDocument`/`GLTFState`; the headless suites never depend on the import cache.
4. The asset's node names are the contract, pinned by test (Frame, Battery, BatteryStrap, Motor1..4, Prop1..4+Blade, Antenna1..2, CameraMount+Lens); blur discs stay in code.
5. Props spin from `motor_rpm` at `SLOWDOWN = 50` of true speed; above `BLUR_RPM = 3000` the blade hides and the disc stands in (both visual estimates, marked in code).
6. Per-family scale = frame arm / 0.08 from the frame part (`quad_spec.gd`); the CameraMount is reset to the FPV eye at authored size so the near clip holds at any scale; unreadable files fall back to the authored 5F.
7. A preview scene in the main menu now; #128's workbench re-hosts it (the seam).
8. FPV unchanged; the chase camera instantiates one scene instead of building ~20 meshes; the frame-rate check stays a by-hand item (#163).
9. The mount's tilt follows the mounted `FpvCamera`'s `uptilt_deg`, else the authored 30°.

## Review Focus

1. **A missing or malformed `quad.gltf` must not take the game down** — the loader's error path leaves a bare node and the flight continues (Task 1, the no-model test).
2. **The near-clip guarantee at every scale** — a 7LR-scaled mount poking past 0.02 m would show nose slivers in FPV; pinned at k = 1.4125 (Task 3).
3. **A stopped motor must look stopped; a windmilling one must turn** — RPM 0 vs RPM > 0 through `set_motors`, both directions per `motor_spin` (Task 2).
4. **A quad whose frame part is missing or lives in a data dir must fall back, not vanish or zero-scale** — `quad_spec.gd` defaults and the library-beside-quads root (Task 3).
5. **The preview must not disturb the keyboard-driven menu** — `mouse_filter = IGNORE`, no focusable children, and the quad row still emits its choice (Task 4).

---

### Task 1: The glTF asset, the runtime loader and the node contract

**Files:**
- Create: `tools/make_quad_model.py`, `godot/drone/quad.gltf` (generated), `godot/drone/QUAD_MODEL.md`, and `godot/drone/quad.gltf.import` if the test runner's `--import` pass writes one (commit it like `godot/ui/osd_font.png.import`)
- Modify: `godot/drone/drone.gd` (rewrite; `set_motors(commands, delta)`, `_props` and the SPIN/DRAWN_SPIN constants stay exactly as they are until Task 2)
- Test: `godot/tests/test_scene.gd` (rewrite `test_the_drone_is_a_modelled_quad_the_fpv_camera_never_sees`, add the no-model test; do not touch the prop test)

**Interfaces:**
- Consumes: nothing new — the numbers come from `godot/drone/drone.gd`'s current procedural build (`ead8a1f`) and `library/frames/opendrone-5f.toml:11-13`.
- Produces: `@export var model_path := "res://drone/quad.gltf"`; `var _model: Node3D` (named `Model`, child of the drone); `_props: Array[Node3D]` filled from the asset's `Prop1..4` pivots; a code-drawn `Disc` (`MeshInstance3D`, translucent cylinder, radius 0.0635) under each pivot; the node names `Frame` (children `Plate`, `PlateTop`, `Arm1`, `Arm2`), `Battery`, `BatteryStrap`, `Motor1..Motor4`, `Prop1..Prop4` (each holding a `Blade`), `Antenna1`, `Antenna2`, `CameraMount` (holding a `Lens`) — Tasks 2-4 and the tests rely on these names.

- [x] **Step 1: Write the failing tests.** In `godot/tests/test_scene.gd`, replace `test_the_drone_is_a_modelled_quad_the_fpv_camera_never_sees` (lines 191-209) with:

```gdscript
func test_the_drone_is_a_gltf_model_the_fpv_camera_never_sees() -> void:
	var drone := Drone.new()
	var camera := FpvCamera.new()
	camera.name = "FpvCamera"
	camera.uptilt_deg = 15.0
	drone.add_child(camera)
	await add_to_tree(drone)
	for part_name in ["Frame", "Plate", "Arm1", "Battery", "BatteryStrap", "Antenna1", "Antenna2", "CameraMount", "Lens", "Blade"]:
		ok(not drone.find_children(part_name, "", true, false).is_empty(), "the asset has a %s" % part_name)
	for i in [1, 2, 3, 4]:
		ok(not drone.find_children("Motor%d" % i, "", true, false).is_empty(), "motor bell %d" % i)
	eq(drone._props.size(), 4, "four propeller pivots")
	var pivot: Vector3 = drone._props[0].position
	ok(absf(pivot.x - 0.08) < 1e-4 and absf(pivot.y - 0.028) < 1e-4 and absf(pivot.z - 0.08) < 1e-4,
		"M1's pivot at the authored rear-right motor: %s" % str(pivot))
	eq(drone.find_children("Disc", "MeshInstance3D", true, false).size(), 4, "a blur disc under every propeller")
	var mounts := drone.find_children("CameraMount", "MeshInstance3D", true, false)
	eq(mounts.size(), 1, "one camera mount")
	eq(mounts[0].position, Vector3(0.0, 0.02, -0.04), "the mount sits at the FPV camera's eye point")
	var corner: float = ((mounts[0] as MeshInstance3D).mesh.get_aabb().size * 0.5).length()
	ok(corner < 0.02, "the mount hides inside the near clip (corner %f)" % corner)
	near(mounts[0].rotation_degrees.x, 15.0, "the mount follows the camera's up-tilt, not the authored 30")
	drone.queue_free()


func test_a_model_that_does_not_load_leaves_a_flying_but_bare_drone() -> void:
	var drone := Drone.new()
	drone.model_path = "res://drone/there-is-no-such-model.gltf"
	await add_to_tree(drone)
	eq(drone._props.size(), 0, "no propeller pivots")
	ok(drone.get_node_or_null("Model") != null, "the fallback node stands in")
	drone.set_motors(PackedFloat32Array([0.0, 0.5, 1.0, 0.5]), 0.01)  # no crash
	drone.queue_free()
```

- [x] **Step 2: Run the suite, verify the new tests fail.** Run: `bash scripts/run-godot-tests.sh unit` (WSL). Expected: FAIL in the two new tests (no `Model` node, `_props` empty; a `push_error` about the missing glTF in the second) and only there — `test_the_drone_has_four_propellers_that_follow_the_motor_commands` still passes (the `set_motors` surface is untouched).

- [x] **Step 3: Write the generator** `tools/make_quad_model.py` (Python 3, stdlib only, deterministic output — the geometry is `drone.gd`'s procedural model carried number-for-number into an asset):

```python
#!/usr/bin/env python3
"""Generates the Godot client's drone model, godot/drone/quad.gltf (provenance: godot/drone/QUAD_MODEL.md).

A glTF 2.0 file: one embedded buffer, one unit-cube mesh per colour (flat baseColorFactor, no textures),
authored in Godot's frame (Y up, forward -Z) at the 5F frame's true size. Regenerate with
`python tools/make_quad_model.py` from the repository root; the output is deterministic, so commit the
script and the asset together.

Usage: python tools/make_quad_model.py [godot/drone/quad.gltf]
"""
import base64
import json
import math
import struct
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "godot/drone/quad.gltf"

CARBON = [0.10, 0.10, 0.11, 1.0]
ARM_COLOR = [0.20, 0.20, 0.22, 1.0]
BATTERY = [0.90, 0.55, 0.10, 1.0]
STRAP = [0.15, 0.15, 0.16, 1.0]
BELL = [0.16, 0.16, 0.18, 1.0]
PROP = [0.08, 0.08, 0.08, 1.0]
ANTENNA = [[0.85, 0.20, 0.20, 1.0], [0.20, 0.40, 0.90, 1.0]]
HOUSING = [0.08, 0.08, 0.09, 1.0]
LENS = [0.02, 0.02, 0.03, 1.0]

EYE = [0.0, 0.02, -0.04]
# Betaflight quad-X in Godot's frame (right +X, back +Z): M1 rear right, M2 front right, M3 rear left, M4 front left.
MOTORS = [[0.08, 0.0, 0.08], [0.08, 0.0, -0.08], [-0.08, 0.0, 0.08], [-0.08, 0.0, -0.08]]


def unit_cube():
    faces = [
        ((0, 0, 1), [(-0.5, -0.5, 0.5), (0.5, -0.5, 0.5), (0.5, 0.5, 0.5), (-0.5, 0.5, 0.5)]),
        ((0, 0, -1), [(0.5, -0.5, -0.5), (-0.5, -0.5, -0.5), (-0.5, 0.5, -0.5), (0.5, 0.5, -0.5)]),
        ((0, 1, 0), [(-0.5, 0.5, 0.5), (0.5, 0.5, 0.5), (0.5, 0.5, -0.5), (-0.5, 0.5, -0.5)]),
        ((0, -1, 0), [(-0.5, -0.5, -0.5), (0.5, -0.5, -0.5), (0.5, -0.5, 0.5), (-0.5, -0.5, 0.5)]),
        ((1, 0, 0), [(0.5, -0.5, 0.5), (0.5, -0.5, -0.5), (0.5, 0.5, -0.5), (0.5, 0.5, 0.5)]),
        ((-1, 0, 0), [(-0.5, -0.5, -0.5), (-0.5, -0.5, 0.5), (-0.5, 0.5, 0.5), (-0.5, 0.5, -0.5)]),
    ]
    positions, normals, indices = [], [], []
    for normal, quad in faces:
        base = len(positions)
        for corner in quad:
            positions += list(corner)
        for _ in quad:
            normals += list(normal)
        indices += [base, base + 1, base + 2, base, base + 2, base + 3]
    return positions, normals, indices


def quat(axis, degrees):
    half = math.radians(degrees) / 2.0
    s, c = math.sin(half), math.cos(half)
    return [s, 0.0, 0.0, c] if axis == "x" else [0.0, s, 0.0, c]


def part(name, mesh, translation, size, rotation=None):
    node = {"name": name, "mesh": mesh, "translation": list(translation), "scale": list(size)}
    if rotation:
        node["rotation"] = rotation
    return node


positions, normals, indices = unit_cube()
blob = struct.pack("<%df" % len(positions), *positions)
blob += struct.pack("<%df" % len(normals), *normals)
blob += struct.pack("<%dH" % len(indices), *indices)
views = [
    {"buffer": 0, "byteOffset": 0, "byteLength": len(positions) * 4},
    {"buffer": 0, "byteOffset": len(positions) * 4, "byteLength": len(normals) * 4},
    {"buffer": 0, "byteOffset": len(positions) * 4 + len(normals) * 4, "byteLength": len(indices) * 2},
]
accessors = [
    {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "max": [0.5, 0.5, 0.5], "min": [-0.5, -0.5, -0.5]},
    {"bufferView": 1, "componentType": 5126, "count": len(normals), "type": "VEC3"},
    {"bufferView": 2, "componentType": 5123, "count": len(indices), "type": "SCALAR", "max": [len(positions) - 1], "min": [0]},
]
colors = [CARBON, ARM_COLOR, BATTERY, STRAP, BELL, PROP, ANTENNA[0], ANTENNA[1], HOUSING, LENS]
materials = [
    {"name": "part%d" % i, "pbrMetallicRoughness": {"baseColorFactor": c, "metallicFactor": 0.0, "roughnessFactor": 0.8}}
    for i, c in enumerate(colors)
]
meshes = [
    {"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": i}]}
    for i in range(len(colors))
]
CARBON_M, ARM_M, BATTERY_M, STRAP_M, BELL_M, PROP_M, ANT1_M, ANT2_M, HOUSING_M, LENS_M = range(len(colors))

tree = [
    ({"name": "Frame"}, [
        (part("Plate", CARBON_M, [0, 0, 0.015], [0.034, 0.005, 0.09]), []),
        (part("PlateTop", CARBON_M, [0, 0.028, 0.012], [0.034, 0.004, 0.085]), []),
        (part("Arm1", ARM_M, [0, 0, 0], [0.24, 0.008, 0.014], quat("y", 45.0)), []),
        (part("Arm2", ARM_M, [0, 0, 0], [0.24, 0.008, 0.014], quat("y", -45.0)), []),
    ]),
    (part("Battery", BATTERY_M, [0, 0.0425, 0.01], [0.035, 0.025, 0.09]), []),
    (part("BatteryStrap", STRAP_M, [0, 0.0425, 0.01], [0.041, 0.028, 0.012]), []),
]
for i, motor in enumerate(MOTORS):
    tree.append((part("Motor%d" % (i + 1), BELL_M, [motor[0], 0.016, motor[2]], [0.052, 0.024, 0.052]), []))
    tree.append(({"name": "Prop%d" % (i + 1), "translation": [motor[0], 0.028, motor[2]]}, [
        (part("Blade", PROP_M, [0, 0, 0], [0.127, 0.002, 0.012]), []),
    ]))
for i, x in enumerate([-0.014, 0.014]):
    tree.append((part("Antenna%d" % (i + 1), ANT1_M if i == 0 else ANT2_M, [x, 0.058, 0.05], [0.007, 0.075, 0.007], quat("x", 40.0)), []))
tree.append((part("CameraMount", HOUSING_M, EYE, [0.02, 0.02, 0.026], quat("x", 30.0)), [
    (part("Lens", LENS_M, [0, 0, -0.015], [0.014, 0.014, 0.004]), []),
]))

nodes, roots = [], []


def walk(entry):
    node, children = entry
    index = len(nodes)
    nodes.append(node)
    node["children"] = [walk(child) for child in children]
    return index


for entry in tree:
    roots.append(walk(entry))

gltf = {
    "asset": {"version": "2.0", "generator": "tools/make_quad_model.py — open-fpv-sim, see godot/drone/QUAD_MODEL.md"},
    "scene": 0,
    "scenes": [{"nodes": roots}],
    "nodes": nodes,
    "meshes": meshes,
    "materials": materials,
    "accessors": accessors,
    "bufferViews": views,
    "buffers": [{"byteLength": len(blob), "uri": "data:application/octet-stream;base64," + base64.b64encode(blob).decode()}],
}
with open(OUT, "w", newline="\n") as f:
    json.dump(gltf, f, indent=1)
    f.write("\n")
print("wrote %s (%d nodes)" % (OUT, len(nodes)))
```

Then write `godot/drone/QUAD_MODEL.md`:

```markdown
# The quad model

`quad.gltf` is the Godot client's drone model — the chase camera and the previews. A glTF 2.0 file: one embedded
buffer, one unit cube per colour, no textures. Regenerate with `python tools/make_quad_model.py` from the
repository root; the output is deterministic, so commit the script and the asset together.

Authored in this repository for the OpenDrone 5" freestyle frame — no third-party art, nothing to licence. The
geometry is the procedural model of PR #147 carried into an asset; the numbers' sources:

- Motor positions and arm, 0.08 m: `library/frames/opendrone-5f.toml` (`frame.motor_positions_frd_m`, an M1
  estimate — its provenance entry applies to this asset too).
- Propeller, 0.127 m: `library/props/opendrone-5in-prop.toml` (`prop.diameter_m`, "5 inch"). Other families'
  prop sizes are drawn as the scaled 5" (a 7" frame shows a 7.1" prop): an estimate, imperceptible at the
  distances the model is seen.
- Plate, arm, bell, battery, strap, antenna and camera sizes and colours: estimates, carried from PR #147's
  procedural model. The camera mount's size is bounded by the FPV camera's 0.02 m near clip, not by hardware.

The node names are the contract `drone.gd` loads by (a test pins them): `Frame` (`Plate`, `PlateTop`, `Arm1`,
`Arm2`), `Battery`, `BatteryStrap`, `Motor1..4`, `Prop1..4` (each holding a `Blade`), `Antenna1..2`,
`CameraMount` (holding a `Lens`). The blur discs, the propeller spin and the per-family scale are drawn in code
(`drone.gd`, `quad_spec.gd`).
```

Run `python tools/make_quad_model.py` and verify `godot/drone/quad.gltf` is valid JSON (`python -c "import json;json.load(open('godot/drone/quad.gltf'))"`) and ~4-6 KB.

- [x] **Step 4: Rewrite `godot/drone/drone.gd`.** New content in full (the doc comment, constants and `set_motors` keep their current shape — `SPIN` and `DRAWN_SPIN` survive until Task 2; `_material` and `_add_box` go — their geometry now lives in the asset):

```gdscript
extends Node3D
## The drone: a glTF quad-X (godot/drone/quad.gltf, authored for the 5F frame — see QUAD_MODEL.md) drawn at the
## pose the simulator reports. Propellers turn with the motor commands.
##
## Godot's frame: forward is -Z, right is +X, up is +Y. The motor order is Betaflight's quad-X (M1 rear right,
## M2 front right, M3 rear left, M4 front left), as in quads/opendrone-5f-freestyle.toml.

## The model's node names are the contract: Frame (Plate, PlateTop, Arm1, Arm2), Battery, BatteryStrap,
## Motor1..4, Prop1..4 (each holding a Blade), Antenna1..2, CameraMount (holding a Lens).
@export var model_path := "res://drone/quad.gltf"
## +1: the propeller turns clockwise seen from above (Betaflight's default direction for each motor).
const SPIN := [1.0, -1.0, -1.0, 1.0]
## A real propeller turns far too fast to show; this is the drawn speed at a full motor command, in rad/s.
const DRAWN_SPIN := 60.0
## The mount's authored up-tilt, used only when no FpvCamera child is mounted to say otherwise.
const AUTHORED_UPTILT_DEG := 30.0
## The FPV camera's eye point (fpv_camera.gd): the camera mount is centred there, wholly inside the camera's
## 0.02 m near clip, so the FPV view never sees its own nose. The chase camera sees the full model.
const FPV_EYE := Vector3(0.0, 0.02, -0.04)

var _model: Node3D
var _props: Array[Node3D] = []


func _ready() -> void:
	_model = _instantiate_model()
	add_child(_model)
	for i in 4:
		var found := _model.find_children("Prop%d" % (i + 1), "Node3D", true, false)
		if found.is_empty():
			push_error("%s has no Prop%d pivot; that propeller cannot turn" % [model_path, i + 1])
			continue
		var pivot := found[0] as Node3D
		_props.append(pivot)
		_add_disc(pivot)
	_apply_uptilt()


## Turns the propellers: `commands` are the motor commands in [0, 1], `delta` the frame time.
func set_motors(commands: PackedFloat32Array, delta: float) -> void:
	for i in mini(_props.size(), commands.size()):
		# Positive rotation about +Y is counter-clockwise seen from above, so clockwise motors turn negatively.
		_props[i].rotate_y(-SPIN[i] * commands[i] * DRAWN_SPIN * delta)


## Loads the model at runtime (GLTFDocument, not the editor's import step: the headless suites never import).
## Re-verify the exact 4.7 signatures in the editor docs at execution; the contract is "the file loads, a
## Node3D tree with the node names above comes back". Any failure leaves a bare node and the game flies on.
func _instantiate_model() -> Node3D:
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	var model: Node3D = null
	if doc.append_from_file(model_path, 0, "", state) == OK:
		model = doc.generate_scene(model_path.get_base_dir(), 0, 30.0, false, true, state) as Node3D
	if model == null:
		push_error("%s did not load; the drone flies as a bare node" % model_path)
		model = Node3D.new()
	model.name = "Model"
	return model


## The blur disc stands in for the propeller at speed; drawn here, not in the asset, so its look stays tunable.
func _add_disc(pivot: Node3D) -> void:
	var disc := MeshInstance3D.new()
	var mesh := CylinderMesh.new()
	mesh.top_radius = 0.0635
	mesh.bottom_radius = 0.0635
	mesh.height = 0.001
	disc.mesh = mesh
	disc.name = "Disc"
	var material := StandardMaterial3D.new()
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.albedo_color = Color(1, 1, 1, 0.12)
	disc.material_override = material
	pivot.add_child(disc)


## The mount's tilt follows the mounted FpvCamera's configured up-tilt (main.tscn mounts one as this node's
## child), so the model's tilt and the camera's cannot drift apart; without a camera the authored tilt stands.
func _apply_uptilt() -> void:
	var found := _model.find_children("CameraMount", "Node3D", true, false)
	if found.is_empty():
		return
	var camera := get_node_or_null("FpvCamera")
	var tilt = camera.get("uptilt_deg") if camera != null else null
	found[0].rotation_degrees.x = tilt if tilt is float else AUTHORED_UPTILT_DEG
```

If the 4.7 `append_from_file`/`generate_scene` signatures differ from the calls above (they grew a `state` parameter in 4.3), follow the editor docs — do not invent a fallback loader; the two tests are the oracle either way.

- [x] **Step 5: Run the suite, verify it passes.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: PASS — the two new tests green, `test_the_drone_has_four_propellers_that_follow_the_motor_commands` still green, every other suite unchanged in failures. If the run's `--import` pass created `godot/drone/quad.gltf.import`, it is committed with the asset.

- [x] **Step 6: Commit**

```bash
git add tools/make_quad_model.py godot/drone/quad.gltf godot/drone/QUAD_MODEL.md godot/drone/drone.gd godot/tests/test_scene.gd
git commit -m "feat(godot): the quad becomes a glTF model (#130)"
```

(add `godot/drone/quad.gltf.import` to the `git add` list if it exists)

### Task 2: Propellers at the simulated motor speeds

**Files:**
- Modify: `godot/drone/drone.gd` (`set_motors`, the constants), `godot/scripts/app.gd:247-248` (the telemetry key)
- Test: `godot/tests/test_scene.gd` (rewrite the prop test, add the blur test)

**Interfaces:**
- Consumes: Task 1's `_props` pivots and `Blade` children; `get_telemetry`'s `motor_rpm` key (`crates/ofs-godot/src/lib.rs:274-275`; re-verify after the #167 rebase).
- Produces: `func set_motors(rpm: PackedFloat32Array, delta: float) -> void` — rpm in revolutions per minute, the state stream's per-motor speed; `const SLOWDOWN := 50.0` and `const BLUR_RPM := 3000.0` (visual estimates, marked in code); `var _blades: Array[Node3D]`. Task 4's preview and the e2e suites need nothing from this task beyond `set_motors` not erroring on short arrays.

- [x] **Step 1: Write the failing tests.** In `godot/tests/test_scene.gd`, replace `test_the_drone_has_four_propellers_that_follow_the_motor_commands` (lines 174-188) with:

```gdscript
func test_the_propellers_follow_the_motor_speeds() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	eq(drone._props.size(), 4, "four propellers")
	var before: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	drone.set_motors(PackedFloat32Array([0.0, 6000.0, 12000.0, 6000.0]), 0.01)
	var after: Array = drone._props.map(func(p: Node3D) -> float: return p.rotation.y)
	near(after[0], before[0], "a stopped motor does not turn its propeller")
	ok(absf(after[2] - before[2]) > absf(after[1] - before[1]), "a faster motor turns its propeller faster")
	# Betaflight's default directions: M1 and M4 clockwise (negative about +Y), M2 and M3 counter-clockwise.
	ok(after[1] > before[1] and after[2] > before[2], "M2 and M3 turn counter-clockwise")
	drone.set_motors(PackedFloat32Array([12000.0, 0.0, 0.0, 12000.0]), 0.01)
	ok(drone._props[0].rotation.y < after[0] and drone._props[3].rotation.y < after[3], "M1 and M4 turn clockwise")
	drone.set_motors(PackedFloat32Array([1000.0]), 0.01)  # fewer speeds than propellers: no error
	drone.queue_free()


func test_a_spinning_propeller_blurs_into_its_disc() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	var blades := drone._props.map(func(p: Node3D) -> Node3D: return p.find_children("Blade", "", true, false)[0])
	drone.set_motors(PackedFloat32Array([1000.0, 0.0, 30000.0, 0.0]), 0.01)
	ok(blades[0].visible, "a slow propeller shows its blade")
	ok(blades[1].visible, "a stopped motor's blade stays visible")
	ok(not blades[2].visible, "a fast one hides it: the blur disc stands in")
	drone.queue_free()
```

- [x] **Step 2: Run the suite, verify the new tests fail.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: FAIL in the two rewritten tests (`set_motors` still expects commands; no blade hiding).

- [x] **Step 3: Implement.** In `godot/drone/drone.gd`: delete the `DRAWN_SPIN` constant and add next to `SPIN`:

```gdscript
## A real propeller turns far too fast to show (30 000 RPM is 500 turns a second); the blade is drawn at this
## fraction of true speed — a visual estimate, tuned so a full-speed 5" draws about as fast as the old model did.
const SLOWDOWN := 50.0
## Above this many RPM the drawn blade would only alias; it hides and the translucent disc stands in. A visual
## estimate: idle and spool-up stay below it, hover and full throttle sit above.
const BLUR_RPM := 3000.0
```

Add `var _blades: Array[Node3D] = []` beside `_props`; in `_ready()`'s loop, after `_add_disc(pivot)`, add `var blades := pivot.find_children("Blade", "Node3D", true, false)` / `if not blades.is_empty():` / `_blades.append(blades[0] as Node3D)` (tab-indented inside the loop). Replace `set_motors` and its doc comment with:

```gdscript
## Turns the propellers: `rpm` are the motor speeds in revolutions per minute from the state stream, `delta` the
## frame time. A stopped motor stops (a failed one shows), a windmilling descent turns; above BLUR_RPM the blade
## hides and the disc stands in.
func set_motors(rpm: PackedFloat32Array, delta: float) -> void:
	for i in mini(_props.size(), rpm.size()):
		# Positive rotation about +Y is counter-clockwise seen from above, so clockwise motors turn negatively.
		_props[i].rotate_y(-SPIN[i] * (rpm[i] / 60.0) * TAU / SLOWDOWN * delta)
		if i < _blades.size():
			_blades[i].visible = rpm[i] < BLUR_RPM
```

Update the file's doc comment: "Propellers turn with the motor commands" becomes "Propellers turn at each motor's simulated speed". In `godot/scripts/app.gd`, replace lines 247-248 (`if telemetry.has("motor_cmd"):` / `drone.set_motors(telemetry["motor_cmd"], delta)`) with:

```gdscript
	if telemetry.has("motor_rpm"):
		drone.set_motors(telemetry["motor_rpm"], delta)
```

- [x] **Step 4: Run the suites, verify they pass.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: PASS, no other suite changed. Then `bash scripts/run-godot-tests.sh open-loop` — the e2e boots the default path with the new key; expected PASS (the drawn pose assertions there do not read the props).

- [x] **Step 5: Commit**

```bash
git add godot/drone/drone.gd godot/scripts/app.gd godot/tests/test_scene.gd
git commit -m "feat(godot): propellers turn at the simulated motor speeds (#130)"
```

### Task 3: The model scales with the quad's frame

**Files:**
- Create: `godot/scripts/quad_spec.gd`, `godot/tests/test_quad_spec.gd`
- Modify: `godot/drone/drone.gd` (`set_quad`, `_spin`), `godot/scripts/app.gd` (two wiring lines), `godot/tests/run_tests.gd` (register the new suite), `godot/tests/test_scene.gd` (one test)
- Test: `godot/tests/test_quad_spec.gd`, `godot/tests/test_scene.gd`

**Interfaces:**
- Consumes: the quad TOMLs' top-level `frame = "id"` (all four shipped quads, `quads/*.toml`) and the frame parts' `motor_positions_frd_m` / `motor_spin` under `[frame]` (`library/frames/*.toml`); the library roots rule of `crates/ofs-config/src/lib.rs:140-155` (a `library/` beside the quads that reference it) — re-verify that function at execution.
- Produces: `QuadSpec.arm_m(quad_path: String) -> float` (the frame's arm in metres, `DEFAULT_ARM_M = 0.08` when unreadable); `QuadSpec.spin(quad_path: String) -> Array` (four floats, `DEFAULT_SPIN = [1.0, -1.0, -1.0, 1.0]` when unreadable); `QuadSpec.frame_part(quad_path: String) -> String`; `QuadSpec.roots(quad_path: String) -> Array`; in `drone.gd`: `const AUTHORED_ARM := 0.08`, `var _spin: Array` (replacing the `SPIN` constant), `func set_quad(path: String) -> void`. Task 4 and the app call `set_quad`.

- [x] **Step 1: Write the failing tests.** Create `godot/tests/test_quad_spec.gd`:

```gdscript
extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")
const QuadSpec = preload("res://scripts/quad_spec.gd")


func test_the_shipped_frames_report_their_arms_and_directions() -> void:
	var root: String = AppSettings.repo_root()
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-3t-toothpick.toml")), 0.044, "the toothpick's 125 mm frame")
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-5f-freestyle.toml")), 0.08, "the freestyle's 220 mm frame")
	near(QuadSpec.arm_m(root.path_join("quads/opendrone-7lr-longrange.toml")), 0.113, "the long-range's 320 mm frame")
	eq(QuadSpec.spin(root.path_join("quads/opendrone-5r-race.toml")), [1.0, -1.0, -1.0, 1.0], "Betaflight quad-X directions")


func test_an_unreadable_quad_falls_back_to_the_authored_5f() -> void:
	var dir := "user://quad-spec-test"
	DirAccess.make_dir_recursive_absolute(dir)
	var path: String = dir.path_join("bare.toml")
	FileAccess.open(path, FileAccess.WRITE).store_string("name = \"Bare\"\n")
	near(QuadSpec.arm_m(path), 0.08, "no frame part: the authored arm")
	eq(QuadSpec.spin(path), [1.0, -1.0, -1.0, 1.0], "the authored directions")
	eq(QuadSpec.arm_m("no-such-file.toml"), 0.08, "a missing quad file too")
	DirAccess.remove_absolute(path)


func test_a_frame_part_beside_its_quads_is_found() -> void:
	var dir := "user://quad-spec-lib"
	DirAccess.make_dir_recursive_absolute(dir.path_join("quads"))
	DirAccess.make_dir_recursive_absolute(dir.path_join("library/frames"))
	FileAccess.open(dir.path_join("quads/x.toml"), FileAccess.WRITE).store_string("frame = \"my-frame\"\n")
	FileAccess.open(dir.path_join("library/frames/my-frame.toml"), FileAccess.WRITE).store_string(
		"[frame]\nmotor_positions_frd_m = [[-0.05, 0.06, 0.0], [0.05, 0.06, 0.0], [-0.05, -0.06, 0.0], [0.05, -0.06, 0.0]]\nmotor_spin = [-1, 1, 1, -1]\n")
	near(QuadSpec.arm_m(dir.path_join("quads/x.toml")), 0.06, "the library beside the quads, like ofs-config")
	eq(QuadSpec.spin(dir.path_join("quads/x.toml")), [-1.0, 1.0, 1.0, -1.0], "reversed directions read too")
```

In `godot/tests/test_scene.gd` add (after the blur test), with `const AppSettings = preload("res://scripts/settings.gd")` beside the other preloads:

```gdscript
func test_the_model_scales_with_the_quad_and_the_mount_stays_at_the_eye() -> void:
	var drone := Drone.new()
	await add_to_tree(drone)
	var root: String = AppSettings.repo_root()
	drone.set_quad(root.path_join("quads/opendrone-7lr-longrange.toml"))
	near(drone.scale.x, 0.113 / 0.08, "the 7\" frame scales the model up", 1e-4)
	var mount: Node3D = drone.find_children("CameraMount", "Node3D", true, false)[0]
	ok(mount.global_position.distance_to(Vector3(0.0, 0.02, -0.04)) < 1e-4, "the mount stays at the FPV eye")
	var corner: float = ((mount as MeshInstance3D).mesh.get_aabb().size * 0.5).length() * mount.global_transform.basis.x.length()
	ok(corner < 0.02, "and inside the near clip at 1.4x scale (corner %f)" % corner)
	drone.set_quad(root.path_join("quads/opendrone-3t-toothpick.toml"))
	near(drone.scale.x, 0.044 / 0.08, "the toothpick scales down", 1e-4)
	drone.set_quad(root.path_join("quads/no-such-quad.toml"))
	near(drone.scale.x, 1.0, "an unreadable quad keeps the authored size", 1e-6)
	drone.queue_free()
```

Register the suite: in `godot/tests/run_tests.gd`'s `SUITES`, add `"res://tests/test_quad_spec.gd",` after `"res://tests/test_scene.gd",`.

- [x] **Step 2: Run the tests, verify they fail.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: FAIL — `res://scripts/quad_spec.gd` does not preload (and `set_quad` does not exist in test_scene's new test).

- [x] **Step 3: Implement.** Create `godot/scripts/quad_spec.gd`:

```gdscript
extends RefCounted
## The geometry a quad's frame part carries, read the way catalog.gd reads a world file: a line scan of the
## TOML, no parser. The simulator resolves library parts through ofs-config's library_roots (a `library/`
## beside the quads that reference it); this is the drawing side's mirror of that rule.

const AppSettings = preload("res://scripts/settings.gd")

## The 5F arm the glTF model is authored for (library/frames/opendrone-5f.toml), and the fallback for anything
## unreadable.
const DEFAULT_ARM_M := 0.08
const DEFAULT_SPIN: Array = [1.0, -1.0, -1.0, 1.0]


## The frame's motor arm in metres (the first motor position's larger sideways coordinate).
static func arm_m(quad_path: String) -> float:
	var row := _first_row(frame_part(quad_path), "motor_positions_frd_m")
	if row.size() < 2:
		return DEFAULT_ARM_M
	return maxf(absf(row[0]), absf(row[1]))


## The frame's propeller directions, +1 clockwise seen from above.
static func spin(quad_path: String) -> Array:
	var values := _numbers(frame_part(quad_path), "motor_spin")
	return Array(values) if values.size() == 4 else DEFAULT_SPIN


## The frame part the quad file references, among the library roots; "" when it is not on disk.
static func frame_part(quad_path: String) -> String:
	var id := _top_level_string(quad_path, "frame")
	if id == "":
		return ""
	for root in roots(quad_path):
		var path: String = String(root).path_join("frames").path_join(id + ".toml")
		if FileAccess.file_exists(path):
			return path
	return ""


## Where the frame part is looked for: the `library/` beside the quad's own directory (ofs-config's rule), then
## the checkout's.
static func roots(quad_path: String) -> Array:
	return [quad_path.get_base_dir().get_base_dir().path_join("library"), AppSettings.repo_root().path_join("library")]


## The value of a top-level `key = "..."` line, read before the first `[section]`, like catalog.gd's name scan.
static func _top_level_string(path: String, key: String) -> String:
	if not FileAccess.file_exists(path):
		return ""
	var table := ""
	for line in FileAccess.get_file_as_string(path).split("\n"):
		var t := line.strip_edges()
		if t.begins_with("["):
			table = t
		elif table == "" and t.begins_with(key) and t.contains("="):
			var pair := t.split("=", true, 1)
			if pair[0].strip_edges() == key:
				return pair[1].strip_edges().trim_prefix("\"").get_slice("\"", 0)
	return ""


## The first `[a, b, c]` row of the line starting with `key` (an array-of-arrays value), as floats.
static func _first_row(path: String, key: String) -> PackedFloat32Array:
	for line in _lines(path):
		var t := line.strip_edges()
		if t.begins_with(key) and t.contains("="):
			var rest := t.split("=", true, 1)[1]
			var start := rest.find("[[")
			var end := rest.find("]")
			if start == -1 or end == -1:
				return PackedFloat32Array()
			return _floats(rest.substr(start + 2, end - start - 2))
	return PackedFloat32Array()


## The numbers of the line starting with `key` (a flat array value), as floats.
static func _numbers(path: String, key: String) -> PackedFloat32Array:
	for line in _lines(path):
		var t := line.strip_edges()
		if t.begins_with(key) and t.contains("="):
			var rest := t.split("=", true, 1)[1]
			var start := rest.find("[")
			var end := rest.rfind("]")
			if start == -1 or end == -1:
				return PackedFloat32Array()
			return _floats(rest.substr(start + 1, end - start - 1))
	return PackedFloat32Array()


static func _lines(path: String) -> PackedStringArray:
	return FileAccess.get_file_as_string(path).split("\n") if FileAccess.file_exists(path) else PackedStringArray()


static func _floats(text: String) -> PackedFloat32Array:
	var out := PackedFloat32Array()
	for part_str in text.split(","):
		out.append(part_str.strip_edges().to_float())
	return out
```

(`FileAccess.get_file_as_string` on a missing file errors into the console — the `file_exists` guard in `_lines` avoids it; `frame_part("")` therefore costs nothing.) In `godot/drone/drone.gd`: add `const QuadSpec = preload("res://scripts/quad_spec.gd")` and `const AUTHORED_ARM := 0.08` at the top; replace `const SPIN := [1.0, -1.0, -1.0, 1.0]` with `## +1: the propeller turns clockwise seen from above; the frame part's motor_spin overrides this (set_quad).` and `var _spin: Array = [1.0, -1.0, -1.0, 1.0]`; change `set_motors`'s `-SPIN[i]` to `-_spin[i]`; and add:

```gdscript
## Scales the model to the quad's frame: the asset is authored for the 5F's arm, so `k` is the frame's own arm
## over that. The camera mount keeps its authored size at the FPV eye point — a bigger frame carries the same
## camera, and the near-clip guarantee survives any scale.
func set_quad(path: String) -> void:
	var k: float = QuadSpec.arm_m(path) / AUTHORED_ARM
	_spin = QuadSpec.spin(path)
	scale = Vector3.ONE * k
	var found: Array = [] if _model == null else _model.find_children("CameraMount", "Node3D", true, false)
	if not found.is_empty():
		var mount := found[0] as Node3D
		mount.position = FPV_EYE / k
		mount.scale = Vector3.ONE / k
```

In `godot/scripts/app.gd`: after `main_menu.setup(settings, overridden)` (in `_ready`, ~line 76) add `drone.set_quad(settings["quad_path"])`, and in `_start_session` after `session.merge(choices, true)` (~line 116) add `drone.set_quad(session["quad_path"])`.

- [x] **Step 4: Run the tests, verify they pass.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: PASS — the three quad_spec tests and the scaling test green; the prop tests still green (`_spin` defaults to the same directions).

- [x] **Step 5: Commit**

```bash
git add godot/scripts/quad_spec.gd godot/tests/test_quad_spec.gd godot/tests/run_tests.gd godot/drone/drone.gd godot/scripts/app.gd godot/tests/test_scene.gd
git commit -m "feat(godot): the model scales with the quad's frame (#130)"
```

### Task 4: A turning quad preview in the main menu

**Files:**
- Create: `godot/ui/quad_preview.gd`, `godot/tests/test_quad_preview.gd`
- Modify: `godot/ui/main_menu.gd` (host the preview, re-point it on quad changes), `godot/tests/run_tests.gd` (register the suite), `godot/tests/test_main_menu.gd` (one test)
- Test: `godot/tests/test_quad_preview.gd`, `godot/tests/test_main_menu.gd`

**Interfaces:**
- Consumes: `Drone.set_quad(path)` (Task 3).
- Produces: `QuadPreview` (a `SubViewportContainer`): `func set_quad(path: String) -> void` (rebuilds the model; "" or unreadable shows the authored 5F), `func model() -> Node3D`; `MainMenu.preview()` returns the widget. This is the unit #128's workbench re-hosts — keep it free of menu-specific behaviour.

- [x] **Step 1: Write the failing tests.** Create `godot/tests/test_quad_preview.gd`:

```gdscript
extends "res://tests/testing.gd"

const AppSettings = preload("res://scripts/settings.gd")
const QuadPreview = preload("res://ui/quad_preview.gd")


func test_the_preview_shows_a_turning_model_of_the_quad() -> void:
	var preview := QuadPreview.new()
	await add_to_tree(preview)
	await tree.process_frame  # the model's _ready
	ok(preview.model() != null and preview.model().get_script() == preload("res://drone/drone.gd"), "a drone model")
	near(preview.model().scale.x, 1.0, "the authored 5F at first")
	ok(preview.mouse_filter == Control.MOUSE_FILTER_IGNORE, "the preview never takes the mouse or the focus")
	var root: String = AppSettings.repo_root()
	preview.set_quad(root.path_join("quads/opendrone-3t-toothpick.toml"))
	await tree.process_frame
	ok(preview.model() != null, "the model was rebuilt")
	near(preview.model().scale.x, 0.044 / 0.08, "at the toothpick's size", 1e-4)
	var before: float = preview.model().global_rotation.y
	preview._process(0.1)
	ok(preview.model().global_rotation.y > before, "the turntable turns the model")
	preview.queue_free()
```

In `godot/tests/test_main_menu.gd` add:

```gdscript
func test_the_menu_previews_the_selected_quad_and_the_choice_still_emits() -> void:
	var m := await _menu()
	var menu = m[0]
	await menu.get_tree().process_frame
	ok(menu.preview() != null and menu.preview().model() != null, "a live preview beside the choices")
	menu.row("quad").set_value(DIR + "/data/quads/zz-b.toml")
	menu.row("quad").changed.emit(DIR + "/data/quads/zz-b.toml")
	eq(m[2].count(["quad_path", DIR + "/data/quads/zz-b.toml"]), 1, "the quad choice still emits once")
	ok(menu.preview().model() != null, "the preview survives a quad change")
	menu.free()
```

Register the suite: in `godot/tests/run_tests.gd`'s `SUITES`, add `"res://tests/test_quad_preview.gd",` after `"res://tests/test_quad_spec.gd",`.

- [x] **Step 2: Run the suite, verify the new tests fail.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: FAIL — `res://ui/quad_preview.gd` does not preload, and the menu has no `preview()`.

- [x] **Step 3: Implement.** Create `godot/ui/quad_preview.gd`:

```gdscript
extends SubViewportContainer
## A turning model of the selected quad, for the main menu now and the workbench later (#128): a small viewport
## with a camera, a light and the drone; set_quad rebuilds the model at the quad's own scale.

const Drone = preload("res://drone/drone.gd")

## How fast the model turns, radians a second.
const TURNTABLE_RADPS := 0.6

var _viewport: SubViewport
var _turntable: Node3D
var _model: Node3D


func _ready() -> void:
	custom_minimum_size = Vector2(280.0, 220.0)
	mouse_filter = MOUSE_FILTER_IGNORE
	stretch = true
	_viewport = SubViewport.new()
	_viewport.transparent_bg = true
	add_child(_viewport)
	var camera := Camera3D.new()
	camera.position = Vector3(0.35, 0.25, 0.45)
	camera.near = 0.01
	camera.look_at_from_position(camera.position, Vector3(0.0, 0.03, 0.0), Vector3.UP)
	_viewport.add_child(camera)
	var light := DirectionalLight3D.new()
	light.rotation_degrees = Vector3(-50.0, -30.0, 0.0)
	_viewport.add_child(light)
	_turntable = Node3D.new()
	_turntable.name = "Turntable"
	_viewport.add_child(_turntable)
	set_quad("")


## Points the preview at a quad file ("" or unreadable: the authored 5F).
func set_quad(path: String) -> void:
	if _model != null:
		_model.queue_free()
	_model = Drone.new()
	_model.name = "Model"
	_turntable.add_child(_model)
	_model.set_quad(path)


## The model shown, for tests.
func model() -> Node3D:
	return _model


func _process(delta: float) -> void:
	_turntable.rotation.y += TURNTABLE_RADPS * delta
```

In `godot/ui/main_menu.gd`: add `const QuadPreview = preload("res://ui/quad_preview.gd")` beside the other preloads and `var _preview := QuadPreview.new()` beside `var _quads`; in `_ready()`, replace the `box` block (lines 44-50: `var box := VBoxContainer.new()` through `add_child(box)`) with:

```gdscript
	var columns := HBoxContainer.new()
	columns.set_anchors_preset(Control.PRESET_CENTER)
	columns.grow_horizontal = Control.GROW_DIRECTION_BOTH
	columns.grow_vertical = Control.GROW_DIRECTION_BOTH
	columns.add_theme_constant_override("separation", 24)
	add_child(columns)
	var box := VBoxContainer.new()
	box.custom_minimum_size.x = 460
	box.add_theme_constant_override("separation", 10)
	box.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	columns.add_child(box)
	columns.add_child(_preview)
	_preview.size_flags_vertical = Control.SIZE_SHRINK_CENTER
```

(the `box.set_anchors_preset(Control.PRESET_CENTER)` / `grow_*` / `box.grow_vertical` lines move to `columns`). Add beside `func hint()`:

```gdscript
## The turning model of the selected quad, for tests (and the workbench, #128).
func preview():
	return _preview
```

Wire the two re-pointings: in `_ready()` after `_quad.changed.connect(...)` (line 67) add `_quad.changed.connect(func(path) -> void: _preview.set_quad(path))`; in `_refresh()` after `_fill(_quad, "Quad", "quad_path", _quads)` (line 125) add `_preview.set_quad(settings["quad_path"])`.

- [x] **Step 4: Run the suite, verify it passes.** Run: `bash scripts/run-godot-tests.sh unit`. Expected: PASS — the two new tests and every existing menu test green (the rows, hints and focus behaviour are untouched by a `MOUSE_FILTER_IGNORE` container).

- [x] **Step 5: Commit**

```bash
git add godot/ui/quad_preview.gd godot/tests/test_quad_preview.gd godot/ui/main_menu.gd godot/tests/run_tests.gd godot/tests/test_main_menu.gd
git commit -m "feat(godot): a turning quad preview in the main menu (#130)"
```

### Task 5: Docs, changelog, the full gate and the by-hand checks

**Files:**
- Modify: `CHANGELOG.md` ("Unreleased"), `README.md` (two paragraphs in "The Godot pilot client")

**Interfaces:**
- Consumes: everything shipped by Tasks 1-4.
- Produces: the documented surface; the by-hand checklist that feeds #163.

- [x] **Step 1: Add the changelog entry.** In `CHANGELOG.md` under `## Unreleased`, after the heightfield section's blocks, add:

```markdown
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
```

- [x] **Step 2: Update the README.** In "The Godot pilot client": after the sentence ending `like real goggles.` (lines 94-97) insert `The drone itself is a glTF model (`godot/drone/quad.gltf`, regenerated by `tools/make_quad_model.py`) that scales with the chosen quad's frame and turns its propellers at the simulated motor speeds.` And in the main-menu paragraph, replace `open loop), then **Start flying**. The menu moves with the arrows or the d-pad and remembers what you chose.` (lines 110-111) with `open loop), then **Start flying**; a small preview beside the choices shows the chosen quad turning. The menu moves with the arrows or the d-pad and remembers what you chose.` Re-verify both anchors against the rebased file first.

- [x] **Step 3: Run the full gate.** `bash scripts/run-godot-tests.sh all` (WSL; the e2e suites start `ofs-sim`). Expected: PASS — every unit suite and the open-loop and errors e2e suites (the betaflight suite runs only with `OFS_SITL_LAUNCH`; note if it ran). No Rust code changed, so no cargo gate applies; run nothing under `cargo` beyond what the test script itself builds.

- [x] **Step 4: Do the by-hand checks** (headless Godot draws nothing; these land as #130's section of #163):
  - The chase camera (**C** from a flight): a recognisable quad-X — plates, X arms, bells, orange battery, two antennas, tilted camera — not boxes; the props spool, blur to discs at speed, and stop with the motors (cut the throttle in a dive: windmilling).
  - The FPV view shows no nose slivers, before and after **R** (reload).
  - The frame rate feels as before on the dev machine (a before/after eyeball of the chase view; the model is one instanced scene, cheaper than the ~20 meshes built per `_ready` before).
  - The menu: the preview turns, and stepping the quad row rescales the model (toothpick to 7LR).
  - Optional, with a display: `godot --path godot -s res://tests/shots.gd -- --out=<dir>` and eyeball `start_chase.png`. If no display, state that it was not done.

- [x] **Step 5: Commit**

```bash
git add CHANGELOG.md README.md
git commit -m "docs: the quad model's changelog entries and README (#130)"
```

## Ledger

- Planned 2026-10-31 against `main` at `87c5914` (worktree `docs/game-completion-plan`). PR #167 merged as `b37aae5` after that base; its `app.gd` hunk (a DVR toast string, ~line 385) does not overlap this plan's edits — re-verified by the executor at rebased execution, per Global Constraints.
- Ruled in planning (spec, calls 2 and 5): the asset's geometry is PR #147's procedural model carried number-for-number — traceability over redesign; `SLOWDOWN = 50` keeps a full-speed 5" drawing at the old `DRAWN_SPIN = 60` rad/s.
