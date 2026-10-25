extends Node3D
## The drone: a modelled quad-X drawn at the pose the simulator reports. Propellers turn with the motor commands.
##
## Godot's frame: forward is -Z, right is +X, up is +Y. The motor order is Betaflight's quad-X (M1 rear right,
## M2 front right, M3 rear left, M4 front left), as in quads/opendrone-5f-freestyle.toml.

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


## Turns the propellers: `commands` are the motor commands in [0, 1], `delta` the frame time.
func set_motors(commands: PackedFloat32Array, delta: float) -> void:
	for i in mini(_props.size(), commands.size()):
		# Positive rotation about +Y is counter-clockwise seen from above, so clockwise motors turn negatively.
		_props[i].rotate_y(-SPIN[i] * commands[i] * DRAWN_SPIN * delta)


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.8
	return material


func _add_box(node_name: String, center: Vector3, size: Vector3, color: Color) -> MeshInstance3D:
	var mesh := BoxMesh.new()
	mesh.size = size
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = center
	add_child(instance)
	return instance
