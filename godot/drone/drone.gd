extends Node3D
## The drone: a small quad drawn at the pose the simulator reports. Propellers turn with the motor commands.
##
## Godot's frame: forward is -Z, right is +X, up is +Y. The motor order is Betaflight's quad-X (M1 rear right,
## M2 front right, M3 rear left, M4 front left), as in quads/opendrone-5f-freestyle.toml.

const ARM := 0.08
const MOTORS := [Vector3(ARM, 0, ARM), Vector3(ARM, 0, -ARM), Vector3(-ARM, 0, ARM), Vector3(-ARM, 0, -ARM)]
## +1: the propeller turns clockwise seen from above (Betaflight's default direction for each motor).
const SPIN := [1.0, -1.0, -1.0, 1.0]
## A real propeller turns far too fast to show; this is the drawn speed at a full motor command, in rad/s.
const DRAWN_SPIN := 60.0

var _props: Array[Node3D] = []


func _ready() -> void:
	_add_box("Body", Vector3(0, 0, 0), Vector3(0.04, 0.03, 0.08), Color(0.12, 0.12, 0.14))
	_add_box("Battery", Vector3(0, 0.025, 0.01), Vector3(0.035, 0.025, 0.09), Color(0.9, 0.55, 0.1))
	for sign_x in [-1.0, 1.0]:
		var arm := _add_box("Arm", Vector3.ZERO, Vector3(0.24, 0.008, 0.014), Color(0.2, 0.2, 0.22))
		arm.rotation_degrees.y = 45.0 * sign_x
	for i in MOTORS.size():
		var pivot := Node3D.new()
		pivot.name = "Prop%d" % (i + 1)
		pivot.position = MOTORS[i] + Vector3(0, 0.012, 0)
		add_child(pivot)
		_props.append(pivot)
		var blade := MeshInstance3D.new()
		var blade_mesh := BoxMesh.new()
		blade_mesh.size = Vector3(0.127, 0.002, 0.012)
		blade.mesh = blade_mesh
		blade.material_override = _material(Color(0.08, 0.08, 0.08))
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