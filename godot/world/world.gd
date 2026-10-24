extends Node3D
## The grey-box world. The sky, the sun and the ground plane with its metre grid are drawn here; the objects on the
## field (launch pad, pylons, gates, buildings), the pilot's spot and other transmitters come from the world file the
## simulator loaded (`OfsClient.get_world()`), so what you see is what the video link sees. Nothing here does the
## physics: the simulator collides the drone with these objects and its ground plane, and the video link's
## occlusion is the simulator's too — this node only draws.
##
## Godot's frame: +Y up, forward (north, where the drone starts facing) is -Z, +X is right (east).

const GRID_SHADER := preload("res://world/grid.gdshader")

const PILOT_COLOR := Color(0.25, 0.55, 1.0)
const EMITTER_COLOR := Color(0.85, 0.25, 0.85)
const GATE_START_COLOR := Color(1.0, 0.85, 0.2)
const GATE_COLOR := Color(0.3, 0.85, 1.0)
const GATE_NEXT_COLOR := Color(0.35, 1.0, 0.45)
const GATE_ALPHA := 0.25
const GATE_NEXT_ALPHA := 0.6
## Where the pilot's eyes are below the goggle position, and how tall the marker is.
const PILOT_BODY_HEIGHT_M := 1.5

## The nodes `build` made, removed by the next `build`.
var _built: Array[Node] = []
## The opening materials of the gates `build` made, in course order, so `set_next_gate` can recolour them.
var _gate_materials: Array[StandardMaterial3D] = []
var _next_gate := -1


func _ready() -> void:
	_add_environment()
	_add_ground()


## Replaces the field's objects and markers with those of `world` (the Dictionary `OfsClient.get_world()` returns;
## an empty one clears the field).
func build(world: Dictionary) -> void:
	for node in _built:
		remove_child(node)
		node.queue_free()
	_built.clear()
	_gate_materials.clear()
	for o in world.get("objects", []):
		match o.get("shape", ""):
			"box":
				_box(o["name"], o["center"], o["size"], o["color"])
			"cylinder":
				_cylinder(o["name"], o["center"], o["radius"], o["height"], o["color"])
	if world.has("pilot_position"):
		_add_pilot(world["pilot_position"], world.get("antennas", []))
	if world.has("handset_position"):
		_add_handset(world["handset_position"])
	for e in world.get("emitters", []):
		_add_emitter(e)
	var gates: Array = world.get("gates", [])
	for i in gates.size():
		_add_gate(i, gates[i])
	set_next_gate(_next_gate)


## Highlights the gate to fly through next: brighter and green, where the others stay translucent cyan and the
## start/finish gate yellow. -1 highlights none (no race, or the race is over).
func set_next_gate(index: int) -> void:
	_next_gate = index
	for i in _gate_materials.size():
		_gate_materials[i].albedo_color = gate_color(i, index)


## The opening's colour: the next gate green and strong, the start/finish gate yellow, the rest cyan.
static func gate_color(index: int, next: int) -> Color:
	if index == next:
		return Color(GATE_NEXT_COLOR.r, GATE_NEXT_COLOR.g, GATE_NEXT_COLOR.b, GATE_NEXT_ALPHA)
	var base := GATE_START_COLOR if index == 0 else GATE_COLOR
	return Color(base.r, base.g, base.b, GATE_ALPHA)


## The names of the nodes the world file made (for the tests and the e2e).
func built_names() -> PackedStringArray:
	var names := PackedStringArray()
	for node in _built:
		names.append(node.name)
	return names


func _add_environment() -> void:
	var sky_material := ProceduralSkyMaterial.new()
	sky_material.sky_top_color = Color(0.30, 0.48, 0.78)
	sky_material.sky_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_horizon_color = Color(0.72, 0.80, 0.88)
	sky_material.ground_bottom_color = Color(0.40, 0.45, 0.40)
	var sky := Sky.new()
	sky.sky_material = sky_material
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = sky
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_energy = 0.8
	var world_environment := WorldEnvironment.new()
	world_environment.environment = environment
	add_child(world_environment)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-50.0, 35.0, 0.0)
	sun.light_energy = 1.1
	sun.shadow_enabled = true
	sun.directional_shadow_max_distance = 150.0
	add_child(sun)


func _add_ground() -> void:
	var plane := PlaneMesh.new()
	plane.size = Vector2(6000.0, 6000.0)
	var material := ShaderMaterial.new()
	material.shader = GRID_SHADER
	var ground := MeshInstance3D.new()
	ground.name = "Ground"
	ground.mesh = plane
	ground.material_override = material
	add_child(ground)


func _material(color: Color) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = color
	material.roughness = 0.9
	return material


func _add_built(node: Node3D) -> void:
	add_child(node)
	_built.append(node)


func _mesh(node_name: String, mesh: Mesh, color: Color, position_: Vector3) -> MeshInstance3D:
	var instance := MeshInstance3D.new()
	instance.name = node_name
	instance.mesh = mesh
	instance.material_override = _material(color)
	instance.position = position_
	return instance


func _box(node_name: String, center: Vector3, size: Vector3, color: Color) -> void:
	var mesh := BoxMesh.new()
	mesh.size = size
	_add_built(_mesh(node_name, mesh, color, center))


func _cylinder(node_name: String, center: Vector3, radius: float, height: float, color: Color) -> void:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	_add_built(_mesh(node_name, mesh, color, center))


## A figure where the pilot stands (its head at the goggles), with a pointer along each patch antenna's aim.
func _add_pilot(goggles: Vector3, antennas: Array) -> void:
	var pilot := Node3D.new()
	pilot.name = "Pilot"
	pilot.position = goggles
	var body := CylinderMesh.new()
	body.top_radius = 0.18
	body.bottom_radius = 0.22
	body.height = PILOT_BODY_HEIGHT_M
	pilot.add_child(_mesh("Body", body, PILOT_COLOR, Vector3(0.0, -0.2 - PILOT_BODY_HEIGHT_M * 0.5, 0.0)))
	var head := SphereMesh.new()
	head.radius = 0.13
	head.height = 0.26
	pilot.add_child(_mesh("Head", head, PILOT_COLOR, Vector3.ZERO))
	for a in antennas:
		if a.get("kind", "") != "patch":
			continue
		var aim: Vector3 = a["aim"]
		var pointer := BoxMesh.new()
		pointer.size = Vector3(0.05, 0.05, 0.8)
		var arrow := _mesh("Aim_" + String(a["name"]), pointer, PILOT_COLOR.lightened(0.4), aim * 0.5)
		arrow.basis = Basis.looking_at(aim, Vector3.UP if absf(aim.y) < 0.99 else Vector3.FORWARD)
		pilot.add_child(arrow)
	_add_built(pilot)


## The pilot's handset: a small dark transmitter, placed at the world dict's handset_position (the config
## default sits it 0.5 m below the goggles).
func _add_handset(position: Vector3) -> void:
	var handset := Node3D.new()
	handset.name = "Handset"
	handset.position = position
	var body := BoxMesh.new()
	body.size = Vector3(0.07, 0.14, 0.025)
	handset.add_child(_mesh("Body", body, Color(0.16, 0.16, 0.2), Vector3.ZERO))
	_add_built(handset)


## A gate's frame in Godot: local +Z along the direction of travel, +Y the opening's "up". The heading is a turn about
## the vertical (north is -Z, east is +X); the optional pitch and roll tilt it after that, aerospace Z-Y-X as in the
## world file (a positive pitch lifts the direction of travel, a positive roll turns the opening clockwise seen
## from the approach). An upright gate is the heading alone.
static func gate_basis(heading_deg: float, pitch_deg: float = 0.0, roll_deg: float = 0.0) -> Basis:
	var h := deg_to_rad(heading_deg)
	var p := deg_to_rad(pitch_deg)
	var r := deg_to_rad(roll_deg)
	var forward_ned := Vector3(cos(h) * cos(p), sin(h) * cos(p), -sin(p))
	var down_ned := Vector3(cos(h) * sin(p) * cos(r) + sin(h) * sin(r), sin(h) * sin(p) * cos(r) - cos(h) * sin(r), cos(p) * cos(r))
	# NED (north, east, down) to Godot (east, up, south).
	var z := Vector3(forward_ned.y, -forward_ned.z, -forward_ned.x)
	var y := Vector3(-down_ned.y, down_ned.z, down_ned.x)
	return Basis(y.cross(z), y, z)


## A gate's opening: a translucent rectangle across the direction of travel (the node's +Z points along it), labelled
## START for gate 0 and with its number for the others. The posts and bar of a physical gate are ordinary objects.
func _add_gate(index: int, g: Dictionary) -> void:
	var gate := Node3D.new()
	gate.name = "Gate_" + String(g["name"])
	gate.position = g["center"]
	gate.basis = gate_basis(float(g["heading_deg"]), float(g.get("pitch_deg", 0.0)), float(g.get("roll_deg", 0.0)))
	var quad := QuadMesh.new()
	quad.size = Vector2(float(g["width"]), float(g["height"]))
	var color := GATE_START_COLOR if index == 0 else GATE_COLOR
	var material := StandardMaterial3D.new()
	_gate_materials.append(material)
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.cull_mode = BaseMaterial3D.CULL_DISABLED
	material.albedo_color = gate_color(index, _next_gate)
	var opening := MeshInstance3D.new()
	opening.name = "Opening"
	opening.mesh = quad
	opening.material_override = material
	gate.add_child(opening)
	var label := Label3D.new()
	label.name = "Label"
	label.text = "START" if index == 0 else str(index)
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.position = Vector3(0.0, float(g["height"]) * 0.5 + 0.35, 0.0)
	label.modulate = color
	gate.add_child(label)
	_add_built(gate)


## A post up to the transmitter, labelled with its frequency.
func _add_emitter(e: Dictionary) -> void:
	var at: Vector3 = e["position"]
	var marker := Node3D.new()
	marker.name = "Emitter_" + String(e["name"])
	marker.position = Vector3(at.x, 0.0, at.z)
	var post := CylinderMesh.new()
	post.top_radius = 0.05
	post.bottom_radius = 0.05
	post.height = maxf(at.y, 0.1)
	marker.add_child(_mesh("Post", post, EMITTER_COLOR, Vector3(0.0, post.height * 0.5, 0.0)))
	var label := Label3D.new()
	label.name = "Label"
	label.text = "%s  %d MHz" % [e["name"], int(e["freq_mhz"])]
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.position = Vector3(0.0, post.height + 0.4, 0.0)
	label.modulate = EMITTER_COLOR
	marker.add_child(label)
	_add_built(marker)
