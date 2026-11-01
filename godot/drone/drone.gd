extends Node3D
## The drone: a glTF quad-X (godot/drone/quad.gltf, authored for the 5F frame — see QUAD_MODEL.md) drawn at the
## pose the simulator reports. Propellers turn at each motor's simulated speed.
##
## Godot's frame: forward is -Z, right is +X, up is +Y. The motor order is Betaflight's quad-X (M1 rear right,
## M2 front right, M3 rear left, M4 front left), as in quads/opendrone-5f-freestyle.toml.

## The model's node names are the contract: Frame (Plate, PlateTop, Arm1, Arm2), Battery, BatteryStrap,
## Motor1..4, Prop1..4 (each holding a Blade), Antenna1..2, CameraMount (holding a Lens). The glTF loader
## uniquifies same-named nodes scene-wide (Blade, Blade2..4), so blades are looked up by the Blade* prefix.
@export var model_path := "res://drone/quad.gltf"
## +1: the propeller turns clockwise seen from above (Betaflight's default direction for each motor).
const SPIN := [1.0, -1.0, -1.0, 1.0]
## A real propeller turns far too fast to show (30 000 RPM is 500 turns a second); the blade is drawn at this
## fraction of true speed — a visual estimate, tuned so a full-speed 5" draws about as fast as the old model did.
const SLOWDOWN := 50.0
## Above this many RPM the drawn blade would only alias; it hides and the translucent disc stands in. A visual
## estimate: idle and spool-up stay below it, hover and full throttle sit above.
const BLUR_RPM := 3000.0
## The mount's authored up-tilt, used only when no FpvCamera child is mounted to say otherwise.
const AUTHORED_UPTILT_DEG := 30.0
## The FPV camera's eye point (fpv_camera.gd): the camera mount is centred there, wholly inside the camera's
## 0.02 m near clip, so the FPV view never sees its own nose. The chase camera sees the full model.
const FPV_EYE := Vector3(0.0, 0.02, -0.04)

var _model: Node3D
var _props: Array[Node3D] = []
var _blades: Array[Node3D] = []


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
		var blades := pivot.find_children("Blade*", "Node3D", true, false)
		if not blades.is_empty():
			_blades.append(blades[0] as Node3D)
	_apply_uptilt()


## Turns the propellers: `rpm` are the motor speeds in revolutions per minute from the state stream, `delta` the
## frame time. A stopped motor stops (a failed one shows), a windmilling descent turns; above BLUR_RPM the blade
## hides and the disc stands in.
func set_motors(rpm: PackedFloat32Array, delta: float) -> void:
	for i in mini(_props.size(), rpm.size()):
		# Positive rotation about +Y is counter-clockwise seen from above, so clockwise motors turn negatively.
		_props[i].rotate_y(-SPIN[i] * (rpm[i] / 60.0) * TAU / SLOWDOWN * delta)
		if i < _blades.size():
			_blades[i].visible = rpm[i] < BLUR_RPM


## Loads the model at runtime (GLTFDocument, not the editor's import step: the headless suites never import).
## The 4.7 signatures (as in world.gd's mesh loading): `append_from_file(path, state)` and `generate_scene(state)`.
## Any failure leaves a bare node and the game flies on.
func _instantiate_model() -> Node3D:
	var doc := GLTFDocument.new()
	var state := GLTFState.new()
	var model: Node3D = null
	if doc.append_from_file(model_path, state) == OK:
		model = doc.generate_scene(state) as Node3D
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
